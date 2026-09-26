//! Stable queue payloads shared by HTTP producers and independently deployed workers.
use kouga_model::Uuid;

/// Kept for jobs persisted before the producer switched to version 2.
#[kouga_job::job(name = "taskboard.task_created", version = 1, queue = "task-mail")]
pub struct TaskCreatedV1 {
    pub task_id: Uuid,
}

#[kouga_job::job(name = "taskboard.task_created", version = 2, queue = "task-mail")]
pub struct TaskCreatedV2 {
    pub task_id: Uuid,
    pub owner_id: Uuid,
}
