//! Transport-independent password, bearer-token and ownership authorization primitives.
//! Apply the SQL migration before using token operations. Applications own their user table.

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use kouga_db::{Db, DbError, DbErrorKind};
use kouga_model::{Column, Model, Query, Uuid};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Hash a password with Argon2id and a new cryptographic random salt.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let mut random_salt = [0_u8; 32];
    random_salt[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    random_salt[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    let salt = SaltString::encode_b64(&random_salt)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
}

/// A malformed stored hash never authenticates.
pub fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentUser {
    pub id: Uuid,
}

fn token_hash(token: &str) -> Option<Vec<u8>> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(Sha256::digest(token.as_bytes()).to_vec())
}

/// The clear token is returned exactly once. Only its SHA-256 digest is stored.
pub async fn issue_token(db: &Db, user_id: Uuid, ttl: Duration) -> Result<String, DbError> {
    let seconds =
        i64::try_from(ttl.as_secs()).map_err(|_| DbError::new(DbErrorKind::InvalidInput))?;
    if seconds == 0 {
        return Err(DbError::new(DbErrorKind::InvalidInput));
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let hash = token_hash(&token).expect("generated token has valid shape");
    // PostgreSQL sets the expiration relative to its own clock, as it does during lookup.
    sqlx::query("INSERT INTO kouga_auth_tokens (token_hash, user_id, expires_at) VALUES ($1, $2, now() + $3 * interval '1 second')")
        .bind(hash)
        .bind(user_id)
        .bind(seconds)
        .execute(db)
        .await
        .map_err(DbError::from)?;
    Ok(token)
}

/// Invalid, expired and revoked tokens all return None; storage errors stay errors.
pub async fn authenticate(db: &Db, token: &str) -> Result<Option<CurrentUser>, DbError> {
    let Some(hash) = token_hash(token) else {
        return Ok(None);
    };
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM kouga_auth_tokens WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(hash)
    .fetch_optional(db)
    .await
    .map_err(DbError::from)?;
    Ok(user_id.map(|id| CurrentUser { id }))
}

pub async fn revoke_token(db: &Db, token: &str) -> Result<bool, DbError> {
    let Some(hash) = token_hash(token) else {
        return Ok(false);
    };
    Ok(sqlx::query("UPDATE kouga_auth_tokens SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(hash)
        .execute(db)
        .await
        .map_err(DbError::from)?
        .rows_affected() > 0)
}

/// Use on password change or reset; all existing sessions for this user are invalidated.
pub async fn revoke_user_tokens(db: &Db, user_id: Uuid) -> Result<u64, DbError> {
    Ok(sqlx::query(
        "UPDATE kouga_auth_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(db)
    .await
    .map_err(DbError::from)?
    .rows_affected())
}

/// Explicit permit is required. Hide maps denial to 404 for private resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Permit,
    Deny,
    Hide,
}

pub fn authorize<A, R>(
    actor: CurrentUser,
    action: &A,
    resource: &R,
    policy: impl FnOnce(CurrentUser, &A, &R) -> Decision,
) -> Result<(), kouga_core::Error> {
    use kouga_core::{Error, ErrorKind};
    match policy(actor, action, resource) {
        Decision::Permit => Ok(()),
        Decision::Deny => Err(Error::new(ErrorKind::Forbidden, "forbidden", "Forbidden")),
        Decision::Hide => Err(Error::new(ErrorKind::NotFound, "not_found", "Not found")),
    }
}

/// Apply this to every list, show and update query before hitting the database.
pub fn owned_by<M: Model>(
    query: Query<M>,
    owner_id: Column<M, Uuid>,
    actor: CurrentUser,
) -> Query<M> {
    query.filter(owner_id.eq(actor.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kouga_core::ErrorKind;

    #[test]
    fn passwords_are_salted_and_reject_wrong_input() {
        let first = hash_password("correct horse").unwrap();
        let second = hash_password("correct horse").unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("correct horse"));
        assert!(verify_password("correct horse", &first));
        assert!(!verify_password("wrong", &first));
        assert!(!verify_password("correct horse", "broken hash"));
    }

    #[test]
    fn policy_defaults_to_denial_and_can_hide() {
        let actor = CurrentUser { id: Uuid::new_v4() };
        let other = Uuid::new_v4();
        assert_eq!(
            authorize(actor, &"show", &other, |_, _, _| Decision::Deny)
                .unwrap_err()
                .kind,
            ErrorKind::Forbidden
        );
        assert_eq!(
            authorize(actor, &"show", &other, |_, _, _| Decision::Hide)
                .unwrap_err()
                .kind,
            ErrorKind::NotFound
        );
        assert!(
            authorize(actor, &"show", &actor.id, |actor, _, owner| {
                if actor.id == *owner {
                    Decision::Permit
                } else {
                    Decision::Deny
                }
            })
            .is_ok()
        );
    }

    #[test]
    fn malformed_tokens_are_not_hashable() {
        assert!(token_hash("not-a-token").is_none());
        assert!(token_hash(&"g".repeat(64)).is_none());
        assert!(token_hash(&"a".repeat(64)).is_some());
    }
}
