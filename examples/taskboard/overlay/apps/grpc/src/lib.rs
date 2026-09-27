use kouga_core::Patch;
use kouga_model::{Db, Uuid};
use std::time::Duration;
use taskboard_board::{Board as DomainBoard, Project, Task};
use taskboard_rpc::rpc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct BoardService {
    db: Db,
    inflight: kouga_grpc::InFlight,
}

impl BoardService {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            inflight: kouga_grpc::InFlight::new(128),
        }
    }

    async fn actor<T>(&self, request: &Request<T>) -> Result<Uuid, Status> {
        Ok(kouga_grpc::require_bearer(request.metadata(), &self.db)
            .await?
            .id)
    }
}

fn id(raw: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(raw).map_err(|_| Status::invalid_argument("Invalid ID"))
}

fn project(value: Project) -> rpc::ProjectReply {
    rpc::ProjectReply {
        id: value.id.to_string(),
        slug: value.slug,
        name: value.name,
    }
}

fn task(value: Task) -> rpc::TaskReply {
    rpc::TaskReply {
        id: value.id.to_string(),
        project_id: value.project_id.to_string(),
        title: value.title,
        completed: value.completed,
    }
}

fn status(error: kouga_core::Error) -> Status {
    kouga_grpc::to_status(error)
}

#[tonic::async_trait]
impl rpc::board_server::Board for BoardService {
    #[tracing::instrument(skip_all, name = "taskboard.grpc.create_project")]
    async fn create_project(
        &self,
        request: Request<rpc::CreateProjectRequest>,
    ) -> Result<Response<rpc::ProjectReply>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = self.actor(&request).await?;
        let input = request.into_inner();
        if input.slug.is_empty()
            || input.slug.len() > 80
            || !input
                .slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || input.name.trim().is_empty()
            || input.name.chars().count() > 100
        {
            return Err(Status::invalid_argument("Invalid project"));
        }
        let board = DomainBoard::new(self.db.clone());
        kouga_grpc::within_deadline(Duration::from_secs(5), async move {
            board
                .create_project(actor, &input.slug, &input.name)
                .await
                .map(project)
                .map(Response::new)
                .map_err(status)
        })
        .await
    }

    #[tracing::instrument(skip_all, name = "taskboard.grpc.create_task")]
    async fn create_task(
        &self,
        request: Request<rpc::CreateTaskRequest>,
    ) -> Result<Response<rpc::TaskReply>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = self.actor(&request).await?;
        let input = request.into_inner();
        let project_id = id(&input.project_id)?;
        if input.title.trim().is_empty() || input.title.chars().count() > 200 {
            return Err(Status::invalid_argument("Invalid title"));
        }
        let board = DomainBoard::new(self.db.clone());
        kouga_grpc::within_deadline(Duration::from_secs(5), async move {
            let created = board.create_task(actor, project_id, &input.title).await;
            // Debug-only fault injection: hold the response after the DB/job
            // transaction commits so a client deadline can be tested safely.
            if cfg!(debug_assertions)
                && std::env::var("KOUGA_ENV").as_deref() == Ok("test")
                && let Ok(ms) = std::env::var("TASKBOARD_TEST_PAUSE_AFTER_GRPC_CREATE_MS")
                && let Ok(ms) = ms.parse::<u64>()
            {
                tokio::time::sleep(Duration::from_millis(ms.min(30_000))).await;
            }
            created.map(task).map(Response::new).map_err(status)
        })
        .await
    }

    #[tracing::instrument(skip_all, name = "taskboard.grpc.get_task")]
    async fn get_task(
        &self,
        request: Request<rpc::GetTaskRequest>,
    ) -> Result<Response<rpc::TaskReply>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = self.actor(&request).await?;
        let task_id = id(&request.into_inner().id)?;
        let board = DomainBoard::new(self.db.clone());
        kouga_grpc::within_deadline(Duration::from_secs(5), async move {
            board
                .task(actor, task_id)
                .await
                .map(task)
                .map(Response::new)
                .map_err(status)
        })
        .await
    }

    #[tracing::instrument(skip_all, name = "taskboard.grpc.complete_task")]
    async fn complete_task(
        &self,
        request: Request<rpc::CompleteTaskRequest>,
    ) -> Result<Response<rpc::TaskReply>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = self.actor(&request).await?;
        let task_id = id(&request.into_inner().id)?;
        let board = DomainBoard::new(self.db.clone());
        kouga_grpc::within_deadline(Duration::from_secs(5), async move {
            board
                .update_task(actor, task_id, Patch::Missing, Patch::Value(true))
                .await
                .map(task)
                .map(Response::new)
                .map_err(status)
        })
        .await
    }

    #[tracing::instrument(skip_all, name = "taskboard.grpc.project_count")]
    async fn get_project_count(
        &self,
        request: Request<rpc::GetProjectCountRequest>,
    ) -> Result<Response<rpc::ProjectCountReply>, Status> {
        let _permit = self.inflight.try_acquire()?;
        let actor = self.actor(&request).await?;
        let project_id = id(&request.into_inner().project_id)?;
        let board = DomainBoard::new(self.db.clone());
        kouga_grpc::within_deadline(Duration::from_secs(5), async move {
            board
                .count(actor, project_id)
                .await
                .map(|value| {
                    Response::new(rpc::ProjectCountReply {
                        total: value.total,
                        completed: value.completed,
                    })
                })
                .map_err(status)
        })
        .await
    }
}
