//! Opt-in smoke test for a separately deployed release Channel image.
use futures_util::{SinkExt, StreamExt};
use std::{process::Command, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

fn curl(args: &[&str], bearer: &str) -> String {
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "-H"])
        .arg(format!("Authorization: Bearer {bearer}"))
        .args(args)
        .output()
        .expect("run local curl");
    assert!(output.status.success(), "local HTTP request failed");
    String::from_utf8(output.stdout).expect("local response is UTF-8")
}

#[tokio::test]
async fn separate_release_channel_receives_owner_task_update() {
    let Ok(channel_url) = std::env::var("T45_CHANNEL_URL") else {
        return;
    };
    let http_url = std::env::var("T45_HTTP_URL").unwrap();
    let token = std::env::var("T45_BEARER").unwrap();
    let owner = std::env::var("T45_OWNER_ID").unwrap();
    let task = std::env::var("T45_TASK_ID").unwrap();
    let ticket_url = format!("{channel_url}/_kouga/ws-ticket");
    let ticket_response = curl(&["-X", "POST", &ticket_url], &token);
    let ticket: serde_json::Value = serde_json::from_str(&ticket_response).unwrap();
    let ticket = ticket["ticket"].as_str().unwrap();
    let mut request = format!("{}/_kouga/ws", channel_url.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", http_url.parse().unwrap());
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("kouga, kouga-ticket.{ticket}").parse().unwrap(),
    );
    let (mut ws, _) = connect_async(request).await.unwrap();
    ws.send(Message::Text(
        serde_json::json!({"type":"subscribe","channel":format!("taskboard.owner.{owner}")})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let ack = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(ack.to_string().contains("subscribed"));
    let task_url = format!("{http_url}/tasks/{task}");
    let update = curl(
        &[
            "-X",
            "PATCH",
            "-H",
            "content-type: application/json",
            "-d",
            r#"{"title":"T45 channel update"}"#,
            &task_url,
        ],
        &token,
    );
    let updated: serde_json::Value = serde_json::from_str(&update).unwrap();
    assert_eq!(updated["data"]["title"], "T45 channel update");
    let event = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(event.to_string().contains("task_changed"));
    assert!(event.to_string().contains(&task));
    let channel = format!("taskboard.owner.{owner}");
    let empty = serde_json::json!({"type":"unsubscribe","channel":channel,"data":""}).to_string();
    let at_limit = serde_json::json!({
        "type":"unsubscribe", "channel":channel, "data":"x".repeat(4096 - empty.len())
    })
    .to_string();
    assert_eq!(at_limit.len(), 4096);
    ws.send(Message::Text(at_limit.into())).await.unwrap();
    ws.send(Message::Text(
        serde_json::json!({"type":"subscribe","channel":channel})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let ack = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(ack.to_string().contains("subscribed"));
    let over_limit = serde_json::json!({
        "type":"unsubscribe", "channel":channel, "data":"x".repeat(4097 - empty.len())
    })
    .to_string();
    assert_eq!(over_limit.len(), 4097);
    ws.send(Message::Text(over_limit.into())).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .unwrap();
    assert!(matches!(
        closed,
        Some(Ok(Message::Close(_))) | None | Some(Err(_))
    ));
}
