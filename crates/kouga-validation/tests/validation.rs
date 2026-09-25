use std::future::Future;
use std::cell::Cell;
use std::task::{Context, Poll, Waker};

use kouga_core::{Error, ErrorKind, Patch};
use kouga_validation::{MAX_ERRORS, Request, Rule, ValidationError, ValidationErrors, validate};

fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

#[test]
fn built_in_rules_handle_unicode_and_inclusive_bounds() {
    let length = Rule::<()>::Length {
        min: Some(2),
        max: Some(3),
    };
    assert!(length.check_length("日本".chars().count()));
    assert!(!length.check_length("日本語文".chars().count()));
    assert!(length.check_length([1, 2, 3].len()));
    assert!(
        Rule::Range {
            min: Some(1),
            max: Some(3)
        }
        .check_range(&3)
    );
    assert!(
        !Rule::Range {
            min: Some(1),
            max: Some(3)
        }
        .check_range(&4)
    );
    assert!(
        Rule::Range {
            min: Some(1.5),
            max: Some(3.5)
        }
        .check_range(&3.5)
    );
    assert!(Rule::<()>::Email.check_email("user@example.com"));
    assert!(!Rule::<()>::Email.check_email("invalid"));
}

#[test]
fn patch_missing_null_and_value_skip_only_absent_values() {
    let missing: Patch<Option<&str>> = Patch::Missing;
    let null: Patch<Option<&str>> = Patch::Value(None);
    let value = Patch::Value(Some("日本"));
    assert!(matches!(missing, Patch::Missing));
    assert!(matches!(null, Patch::Value(None)));
    assert!(matches!(value, Patch::Value(Some(_))));
}

#[test]
fn errors_keep_order_and_stop_at_cap() {
    let mut errors = ValidationErrors::default();
    for i in 0..MAX_ERRORS + 1 {
        errors.push(ValidationError::new(format!("rule_{i}")).at("items[0].name"));
    }
    assert_eq!(errors.len(), MAX_ERRORS);
    assert_eq!(errors.as_slice()[0].code, "rule_0");
    assert_eq!(errors.as_slice()[MAX_ERRORS - 1].code, "rule_99");
    assert!(errors.is_full());
}

#[derive(Debug)]
struct Input {
    fail_sync: bool,
    fail_async: bool,
}

thread_local! {
    static ASYNC_CALLS: Cell<usize> = const { Cell::new(0) };
}

impl Request for Input {
    type Context = bool;

    fn validate_sync(&self, errors: &mut ValidationErrors) {
        if self.fail_sync {
            errors.push(ValidationError::new("invalid").at("name"));
        }
    }

    async fn validate_async<'a>(
        &'a self,
        infrastructure_down: &'a Self::Context,
        errors: &'a mut ValidationErrors,
    ) -> Result<(), Error> {
        ASYNC_CALLS.with(|calls| calls.set(calls.get() + 1));
        if *infrastructure_down {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "db_unavailable",
                "Unavailable",
            ));
        }
        if self.fail_async {
            errors.push(ValidationError::new("taken").at("slug"));
        }
        Ok(())
    }
}

#[test]
fn sync_errors_skip_async_and_async_errors_are_validation() {
    ASYNC_CALLS.with(|calls| calls.set(0));
    let error = ready(validate(
        Input {
            fail_sync: true,
            fail_async: false,
        },
        &false,
    ))
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(error.details[0].field, "name");
    assert_eq!(ASYNC_CALLS.with(Cell::get), 0);

    let error = ready(validate(
        Input {
            fail_sync: false,
            fail_async: true,
        },
        &false,
    ))
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(error.details[0].code, "taken");
    assert_eq!(ASYNC_CALLS.with(Cell::get), 1);
}

#[test]
fn infrastructure_error_is_not_disguised_as_bad_input() {
    let error = ready(validate(
        Input {
            fail_sync: false,
            fail_async: false,
        },
        &true,
    ))
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    let validated = ready(validate(
        Input {
            fail_sync: false,
            fail_async: false,
        },
        &false,
    ))
    .unwrap();
    assert!(!validated.as_ref().fail_sync);
    assert!(!validated.into_inner().fail_async);
}
