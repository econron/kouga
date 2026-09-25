use kouga_core::Error;
use kouga_validation::{Request, ValidationErrors};

struct Input;

impl Request for Input {
    type Context = ();

    fn validate_sync(&self, _: &mut ValidationErrors) {}

    async fn validate_async<'a>(
        &'a self,
        _: &'a Self::Context,
        _: &'a mut ValidationErrors,
    ) -> Result<(), Error> {
        Ok(())
    }
}

fn accepts_request<T: Request<Context = ()>>() {}

#[test]
fn request_signature_accepts_plain_async_function() {
    accepts_request::<Input>();
}
