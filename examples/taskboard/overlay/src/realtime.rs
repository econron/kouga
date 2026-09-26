//! Owner-specific channels. Task events are emitted in the mutation transaction.
use kouga_auth::CurrentUser;
use kouga_channel::{Action, ChannelError};
use kouga_model::{Uuid, sqlx};

pub fn owner_channel(owner: Uuid) -> String {
    format!("taskboard.owner.{owner}")
}

pub fn policy(actor: CurrentUser, action: Action, channel: &str) -> bool {
    action == Action::Subscribe && channel == owner_channel(actor.id)
}

pub async fn task_changed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    task: Uuid,
    action: &'static str,
) -> Result<(), ChannelError> {
    kouga_channel::publish_in(
        tx,
        &owner_channel(owner),
        serde_json::json!({"type":"task_changed","action":action,"task_id":task}),
    )
    .await
}
