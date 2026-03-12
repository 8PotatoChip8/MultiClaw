use anyhow::Result;
use axum::{
    extract::Query,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn start_server() -> Result<()> {
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/exec", post(exec_handler))
        .route("/file/read", get(file_read_handler))
        .route("/file/write", post(file_write_handler))
        .route("/file/list", get(file_list_handler));

    // Bind to loopback port reserved for sidecar control
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18790").await?;
    tracing::info!("Agentd local API server listening on 127.0.0.1:18790");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> Json<Value> {
    Json(json!({"status": "ok", "service": "multiclaw-agentd"}))
}

// ── Command Execution ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ExecRequest {
    command: String,
    working_dir: Option<String>,
    timeout_secs: Option<u64>,
}

async fn exec_handler(Json(body): Json<ExecRequest>) -> Json<Value> {
    let wd = body.working_dir.unwrap_or_else(|| "/home/ubuntu".into());
    let secs = body.timeout_secs.unwrap_or(30).min(120);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        tokio::process::Command::new("sh")
            .args(["-c", &body.command])
            .current_dir(&wd)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
            // Truncate to 1MB
            stdout.truncate(1_048_576);
            stderr.truncate(1_048_576);
            Json(json!({
                "exit_code": output.status.code().unwrap_or(-1),
                "stdout": stdout,
                "stderr": stderr,
            }))
        }
        Ok(Err(e)) => Json(json!({
            "exit_code": -1,
            "stdout": "",
            "stderr": e.to_string(),
        })),
        Err(_) => Json(json!({
            "exit_code": -1,
            "stdout": "",
            "stderr": format!("Command timed out after {} seconds", secs),
        })),
    }
}

// ── File Operations ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct FilePathQuery {
    path: String,
}

async fn file_read_handler(Query(q): Query<FilePathQuery>) -> Json<Value> {
    match tokio::fs::read(&q.path).await {
        Ok(content) => {
            let text = String::from_utf8_lossy(&content).to_string();
            Json(json!({
                "path": q.path,
                "content": text,
                "size": content.len(),
            }))
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Debug, Deserialize)]
struct FileWriteRequest {
    path: String,
    content: String,
}

async fn file_write_handler(Json(body): Json<FileWriteRequest>) -> Json<Value> {
    match tokio::fs::write(&body.path, body.content.as_bytes()).await {
        Ok(()) => Json(json!({"status": "ok", "path": body.path})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn file_list_handler(Query(q): Query<FilePathQuery>) -> Json<Value> {
    match tokio::fs::read_dir(&q.path).await {
        Ok(mut entries) => {
            let mut files = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let meta = entry.metadata().await.ok();
                files.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                    "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                }));
            }
            Json(json!({"path": q.path, "entries": files}))
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}
