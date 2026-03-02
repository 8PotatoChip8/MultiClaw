use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use super::ws::{events_handler, AppState};

// These are mock handlers for the MVP. In a full implementation, they would wire 
// to sqlx queries, the policy engine, and the dispatcher.

pub fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/install/init", post(handle_init))
        .route("/v1/companies", get(list_companies).post(create_company))
        .route("/v1/companies/:id/org-tree", get(get_org_tree))
        .route("/v1/companies/:id/hire-ceo", post(hire_ceo))
        .route("/v1/agents/:id/hire-manager", post(hire_manager))
        .route("/v1/agents/:id/hire-worker", post(hire_worker))
        .route("/v1/threads/:id/messages", post(send_message))
        .route("/v1/requests/:id/approve", post(approve_request))
        .route("/v1/requests/:id/reject", post(reject_request))
        .route("/v1/events", get(events_handler))
        .with_state(state)
}

async fn handle_init() -> impl IntoResponse {
    // 1. Create Holding Company
    // 2. Create MainAgent DB row
    // 3. Create tool_policies
    (StatusCode::OK, Json(json!({"status": "success"})))
}

async fn list_companies() -> impl IntoResponse {
    (StatusCode::OK, Json(json!([])))
}

async fn create_company() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"id": uuid::Uuid::new_v4(), "status": "created"})))
}

async fn get_org_tree(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"tree": {}})))
}

async fn hire_ceo(Path(_id): Path<String>) -> impl IntoResponse {
    // 1. Invoke Policy Engine `can_hire_second_ceo`
    // 2. If immediate, create DB rows and call `Provisioning::provision`
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn hire_manager(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn hire_worker(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status": "hiring_initiated"})))
}

async fn send_message(Path(_id): Path<String>, Json(_payload): Json<Value>) -> impl IntoResponse {
    // 1. Dispatch message
    (StatusCode::OK, Json(json!({"status": "dispatched"})))
}

async fn approve_request(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "approved"})))
}

async fn reject_request(Path(_id): Path<String>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "rejected"})))
}
