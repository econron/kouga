use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use kouga_model::{Uuid, sqlx};
use kouga_test::{TestClient, TestDb};
use std::process::Command;

async fn send(
    client: &TestClient,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    client
        .send(
            request
                .extension(ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    41335,
                ))))
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
}
async fn body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}
async fn register(client: &TestClient, email: &str) -> (Uuid, String) {
    let response = send(
        client,
        "POST",
        "/auth/register",
        &format!(r#"{{"email":"{email}","password":"correct horse battery"}}"#),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let output = body(response).await;
    (
        Uuid::parse_str(output["data"]["user"]["id"].as_str().unwrap()).unwrap(),
        output["data"]["token"].as_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn owner_scope_validation_constraints_and_cache() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let client = TestClient::new(taskboard::router().with_state(isolated.db().clone()));
    let (alice, alice_token) = register(&client, "alice@example.com").await;
    let (bob, bob_token) = register(&client, "bob@example.com").await;
    assert_eq!(
        send(
            &client,
            "POST",
            "/projects",
            r#"{"slug":"x","name":""}"#,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let project = send(
        &client,
        "POST",
        "/projects",
        r#"{"slug":"demo","name":"Demo"}"#,
        Some(&alice_token),
    )
    .await;
    assert_eq!(project.status(), StatusCode::CREATED);
    let project_id = body(project).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/projects/{project_id}");
    for (method, target, payload) in [
        ("GET", path.as_str(), ""),
        ("PATCH", path.as_str(), r#"{"name":"no"}"#),
        ("DELETE", path.as_str(), ""),
    ] {
        assert_eq!(
            send(&client, method, target, payload, Some(&bob_token))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let response = send(&client, "GET", "/projects", "", Some(&bob_token)).await;
    assert_eq!(body(response).await["data"].as_array().unwrap().len(), 0);
    assert_eq!(
        send(
            &client,
            "POST",
            "/tasks",
            &format!(r#"{{"project_id":"{project_id}","title":"hidden"}}"#),
            Some(&bob_token)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let invalid = send(
        &client,
        "POST",
        "/projects",
        r#"{"slug":"bad","name":"","owner_id":"not-allowed"}"#,
        Some(&alice_token),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let invalid = send(
        &client,
        "POST",
        "/projects",
        r#"{"slug":"bad","name":"   "}"#,
        Some(&alice_token),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE owner_id=$1")
        .bind(alice)
        .fetch_one(isolated.db())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let input = format!(r#"{{"project_id":"{project_id}","title":"first"}}"#);
    let created = send(&client, "POST", "/tasks", &input, Some(&alice_token)).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let task_id = body(created).await["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task_path = format!("/tasks/{task_id}");
    assert_eq!(
        send(&client, "GET", &task_path, "", Some(&bob_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &client,
            "PATCH",
            &task_path,
            r#"{"completed":true}"#,
            Some(&bob_token)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&client, "DELETE", &task_path, "", Some(&bob_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        body(send(&client, "GET", "/tasks", "", Some(&bob_token)).await).await["data"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let count_path = format!("/projects/{project_id}/count");
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["total"],
        1
    );
    assert_eq!(
        send(&client, "GET", &count_path, "", Some(&bob_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let updated = send(
        &client,
        "PATCH",
        &task_path,
        r#"{"completed":true}"#,
        Some(&alice_token),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["completed"],
        1
    );
    assert_eq!(
        send(
            &client,
            "PATCH",
            &task_path,
            r#"{"completed":false}"#,
            Some(&alice_token)
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let page = body(
        send(
            &client,
            "GET",
            "/tasks?page=1&per_page=1",
            "",
            Some(&alice_token),
        )
        .await,
    )
    .await;
    assert_eq!(page["data"].as_array().unwrap().len(), 1);
    assert_eq!(page["data"][0]["project_name"], "Demo");
    assert_eq!(
        send(
            &client,
            "PATCH",
            &task_path,
            r#"{"title":""}"#,
            Some(&alice_token)
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        send(
            &client,
            "PATCH",
            &task_path,
            r#"{"owner_id":"x"}"#,
            Some(&alice_token)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let stored_title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id=$1")
        .bind(Uuid::parse_str(&task_id).unwrap())
        .fetch_one(isolated.db())
        .await
        .unwrap();
    assert_eq!(stored_title, "first");
    let direct = taskboard::board::Board::new(isolated.db().clone());
    assert!(
        direct
            .create_task(alice, Uuid::parse_str(&project_id).unwrap(), "  ")
            .await
            .is_err()
    );
    assert!(
        direct
            .update_task(
                alice,
                Uuid::parse_str(&task_id).unwrap(),
                kouga_core::Patch::Missing,
                kouga_core::Patch::Value(false)
            )
            .await
            .is_err()
    );
    let second = body(
        send(
            &client,
            "POST",
            "/tasks",
            &format!(r#"{{"project_id":"{project_id}","title":"second"}}"#),
            Some(&alice_token),
        )
        .await,
    )
    .await;
    let second_id = Uuid::parse_str(second["data"]["id"].as_str().unwrap()).unwrap();
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(isolated.db())
        .await
        .unwrap();
    let runner_url = format!(
        "{}?options=-csearch_path%3D{schema}",
        std::env::var("TEST_DATABASE_URL").unwrap()
    );
    let run = |actor: Uuid| {
        Command::new(env!("CARGO_BIN_EXE_task-complete"))
            .env("DATABASE_URL", &runner_url)
            .env("BOARD_ACTOR_ID", actor.to_string())
            .env("BOARD_TASK_ID", second_id.to_string())
            .output()
            .unwrap()
    };
    assert!(!run(bob).status.success());
    assert!(run(alice).status.success());
    let page = body(
        send(
            &client,
            "GET",
            "/tasks?page=1&per_page=1",
            "",
            Some(&alice_token),
        )
        .await,
    )
    .await;
    assert_eq!(page["meta"]["has_next"], true);
    assert_eq!(page["data"].as_array().unwrap().len(), 1);
    let next = body(
        send(
            &client,
            "GET",
            "/tasks?page=2&per_page=1",
            "",
            Some(&alice_token),
        )
        .await,
    )
    .await;
    assert_eq!(next["meta"]["has_next"], false);
    assert_eq!(next["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["total"],
        2
    );
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["completed"],
        2
    );
    direct.delete_task(alice, second_id).await.unwrap();
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["total"],
        1
    );
    assert_eq!(
        send(&client, "DELETE", &task_path, "", Some(&alice_token))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(send(&client, "GET", &count_path, "", Some(&alice_token)).await).await["data"]["total"],
        0
    );
    let updated_project = send(
        &client,
        "PATCH",
        &path,
        r#"{"name":"Renamed"}"#,
        Some(&alice_token),
    )
    .await;
    assert_eq!(updated_project.status(), StatusCode::OK);
    assert_eq!(body(updated_project).await["data"]["name"], "Renamed");
    assert_eq!(
        send(&client, "DELETE", &path, "", Some(&alice_token))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&client, "GET", &path, "", Some(&alice_token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    isolated.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_duplicates_and_foreign_key() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }
    let isolated = TestDb::from_env("migrations").await.unwrap();
    let client = TestClient::new(taskboard::router().with_state(isolated.db().clone()));
    let (alice, token) = register(&client, "parallel@example.com").await;
    let duplicate = r#"{"slug":"same","name":"A"}"#;
    let (one, two) = tokio::join!(
        send(&client, "POST", "/projects", duplicate, Some(&token)),
        send(&client, "POST", "/projects", duplicate, Some(&token))
    );
    assert!([one.status(), two.status()].contains(&StatusCode::CREATED));
    assert!([one.status(), two.status()].contains(&StatusCode::CONFLICT));
    let conflict = if one.status() == StatusCode::CONFLICT {
        one
    } else {
        two
    };
    assert_eq!(body(conflict).await["error"]["code"], "duplicate");
    let project_id: Uuid = sqlx::query_scalar("SELECT id FROM projects WHERE owner_id=$1")
        .bind(alice)
        .fetch_one(isolated.db())
        .await
        .unwrap();
    let input = format!(r#"{{"project_id":"{project_id}","title":"same"}}"#);
    let (one, two) = tokio::join!(
        send(&client, "POST", "/tasks", &input, Some(&token)),
        send(&client, "POST", "/tasks", &input, Some(&token))
    );
    assert!([one.status(), two.status()].contains(&StatusCode::CREATED));
    assert!([one.status(), two.status()].contains(&StatusCode::CONFLICT));
    let conflict = if one.status() == StatusCode::CONFLICT {
        one
    } else {
        two
    };
    assert_eq!(body(conflict).await["error"]["code"], "duplicate");
    let other_owner = Uuid::new_v4();
    let fk =
        sqlx::query("INSERT INTO tasks(id,project_id,owner_id,title) VALUES ($1,$2,$3,'spoof')")
            .bind(Uuid::new_v4())
            .bind(project_id)
            .bind(other_owner)
            .execute(isolated.db())
            .await
            .unwrap_err();
    assert_eq!(
        fk.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE project_id=$1")
        .bind(project_id)
        .fetch_one(isolated.db())
        .await
        .unwrap();
    assert_eq!(total, 1);
    isolated.close().await.unwrap();
}
