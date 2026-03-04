use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, patch},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
use tower_http::cors::{CorsLayer, Any};

use super::ws::{events_handler, AppState};
use crate::db::models::*;

pub fn app_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health & Install
        .route("/v1/health", get(health))
        .route("/v1/install/init", post(handle_init))
        // Companies
        .route("/v1/companies", get(list_companies).post(create_company))
        .route("/v1/companies/:id", get(get_company).patch(update_company))
        .route("/v1/companies/:id/org-tree", get(get_org_tree))
        .route("/v1/companies/:id/hire-ceo", post(hire_ceo))
        .route("/v1/companies/:id/ledger", get(get_ledger))
        // Agents
        .route("/v1/agents", get(list_agents))
        .route("/v1/agents/:id", get(get_agent).patch(patch_agent))
        .route("/v1/agents/:id/hire-manager", post(hire_manager))
        .route("/v1/agents/:id/hire-worker", post(hire_worker))
        .route("/v1/agents/:id/vm/start", post(vm_start))
        .route("/v1/agents/:id/vm/stop", post(vm_stop))
        .route("/v1/agents/:id/vm/rebuild", post(vm_rebuild))
        .route("/v1/agents/:id/panic", post(agent_panic))
        .route("/v1/agents/:id/thread", get(get_agent_thread))
        // Threads & Messages
        .route("/v1/threads", get(get_threads).post(create_thread))
        .route("/v1/threads/:id", get(get_thread))
        .route("/v1/threads/:id/messages", get(get_messages).post(send_message))
        .route("/v1/threads/:id/participants", get(get_thread_participants))
        // Requests & Approvals
        .route("/v1/requests", get(list_requests).post(create_request))
        .route("/v1/requests/:id/approve", post(approve_request))
        .route("/v1/requests/:id/reject", post(reject_request))
        // Services
        .route("/v1/services", get(list_services).post(create_service))
        .route("/v1/engagements", post(create_engagement))
        .route("/v1/engagements/:id/activate", post(activate_engagement))
        .route("/v1/engagements/:id/complete", post(complete_engagement))
        // Agentd
        .route("/v1/agentd/register", post(agentd_register))
        .route("/v1/agentd/heartbeat", post(agentd_heartbeat))
        // System
        .route("/v1/system/update-check", get(system_update_check))
        .route("/v1/system/update", post(system_update))
        // Events WS
        .route("/v1/events", get(events_handler))
        .layer(cors)
        .with_state(state)
}

// ═══════════════════════════════════════════════════════════════
// Health
// ═══════════════════════════════════════════════════════════════

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

// ═══════════════════════════════════════════════════════════════
// Install Init
// ═══════════════════════════════════════════════════════════════

async fn handle_init(
    State(state): State<AppState>,
    Json(payload): Json<InitRequest>,
) -> impl IntoResponse {
    let holding_name = payload.holding_name.unwrap_or_else(|| "Main Holding".into());
    let agent_name = payload.main_agent_name.unwrap_or_else(|| "MainAgent".into());
    let model = payload.default_model.unwrap_or_else(|| "glm-5:cloud".into());

    let holding_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();

    // Check if already initialized
    let existing: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM holdings")
        .fetch_optional(&state.db).await.unwrap_or(None);
    if let Some((count,)) = existing {
        if count > 0 {
            return (StatusCode::OK, Json(json!({"status": "already_initialized"})));
        }
    }

    // Create holding
    if let Err(e) = sqlx::query(
        "INSERT INTO holdings (id, owner_user_id, name, main_agent_name) VALUES ($1, $2, $3, $4)"
    ).bind(holding_id).bind(owner_id).bind(&holding_name).bind(&agent_name)
    .execute(&state.db).await {
        tracing::error!("Failed to create holding: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})));
    }

    // Create default tool policies
    let ceo_policy_id = Uuid::new_v4();
    let mgr_policy_id = Uuid::new_v4();
    let wkr_policy_id = Uuid::new_v4();
    let main_policy_id = Uuid::new_v4();

    for (id, name, allow, deny) in [
        (main_policy_id, "main_agent_policy", json!(["*"]), json!([])),
        (ceo_policy_id, "ceo_policy", json!(["browser","files","coding","sessions"]), json!(["vm_provisioning"])),
        (mgr_policy_id, "manager_policy", json!(["browser","files"]), json!(["system.run"])),
        (wkr_policy_id, "worker_policy", json!(["browser","files"]), json!(["system.run","admin"])),
    ] {
        let _ = sqlx::query(
            "INSERT INTO tool_policies (id, name, allowlist, denylist, notes) VALUES ($1, $2, $3, $4, $5)"
        ).bind(id).bind(name).bind(&allow).bind(&deny).bind("Default policy")
        .execute(&state.db).await;
    }

    // Create MainAgent
    let agent_id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, effective_model, system_prompt, tool_policy_id, status) \
         VALUES ($1, $2, NULL, 'MAIN', $3, 'Holding Company Management', $4, $5, $6, 'ACTIVE')"
    )
    .bind(agent_id).bind(holding_id).bind(&agent_name).bind(&model)
    .bind(format!("You are {}, the Main Agent managing this holding company.", agent_name))
    .bind(main_policy_id)
    .execute(&state.db).await;

    tracing::info!("Initialized holding '{}' with MainAgent '{}'", holding_name, agent_name);
    (StatusCode::OK, Json(json!({"status": "success", "holding_id": holding_id, "main_agent_id": agent_id})))
}

// ═══════════════════════════════════════════════════════════════
// Companies
// ═══════════════════════════════════════════════════════════════

async fn list_companies(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies ORDER BY created_at DESC"
    ).fetch_all(&state.db).await {
        Ok(c) => (StatusCode::OK, Json(json!(c))),
        Err(e) => { tracing::error!("list_companies: {}", e); (StatusCode::OK, Json(json!([]))) }
    }
}

async fn get_company(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(c)) => (StatusCode::OK, Json(json!(c))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error":"Not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn create_company(State(state): State<AppState>, Json(payload): Json<CreateCompanyRequest>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let holding: Option<Holding> = sqlx::query_as("SELECT id, owner_user_id, name, main_agent_name, created_at FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let holding_id = holding.map(|h| h.id).unwrap_or(Uuid::from_u128(0));

    match sqlx::query_as::<_, Company>(
        "INSERT INTO companies (id, holding_id, name, type, description, status) VALUES ($1,$2,$3,$4,$5,'ACTIVE') \
         RETURNING id, holding_id, name, type, description, tags, status, created_at"
    ).bind(id).bind(holding_id).bind(&payload.name).bind(&payload.r#type).bind(&payload.description)
    .fetch_one(&state.db).await {
        Ok(c) => {
            let _ = state.tx.send(json!({"type":"company_created","company": c}).to_string());

            // Autonomous org bootstrapping: ask MainAgent to hire staff
            let company_name = payload.name.clone();
            let company_desc = payload.description.clone().unwrap_or_else(|| "general operations".into());
            let company_id = id;
            let state_clone = state.clone();

            tokio::spawn(async move {
                tracing::info!("Starting autonomous org bootstrap for company '{}' ({})", company_name, company_id);

                let prompt = format!(
                    "A new company called '{}' has been created with purpose: '{}'. \
                     Its ID is {}. Please do the following:\n\
                     1. Hire a CEO for this company with an appropriate name and specialty.\n\
                     2. Hire 2 managers with different specialties relevant to the company's purpose.\n\
                     3. Hire 1 worker under the company for initial operations.\n\
                     Execute these hires now using your tools.",
                    company_name, company_desc, company_id
                );

                match state_clone.main_agent.handle_message(&state_clone.db, &prompt).await {
                    Ok(response) => {
                        tracing::info!("Org bootstrap complete for '{}': {}", company_name, &response[..response.len().min(200)]);
                        // Broadcast an event so the UI updates
                        let _ = state_clone.tx.send(json!({
                            "type": "org_bootstrap_complete",
                            "company_id": company_id,
                            "summary": response
                        }).to_string());
                    }
                    Err(e) => {
                        tracing::error!("Org bootstrap failed for '{}': {}", company_name, e);
                    }
                }
            });

            (StatusCode::CREATED, Json(json!(c)))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Org Tree
// ═══════════════════════════════════════════════════════════════

async fn get_org_tree(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, handle, status, created_at \
         FROM agents WHERE company_id = $1 ORDER BY role, name"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(agents) => (StatusCode::OK, Json(json!({"company_id": uid, "tree": agents}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Agents
// ═══════════════════════════════════════════════════════════════

async fn list_agents(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, handle, status, created_at \
         FROM agents ORDER BY created_at"
    ).fetch_all(&state.db).await {
        Ok(a) => (StatusCode::OK, Json(json!(a))),
        Err(e) => { tracing::error!("list_agents: {}", e); (StatusCode::OK, Json(json!([]))) }
    }
}

async fn get_agent(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, handle, status, created_at \
         FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(a)) => (StatusCode::OK, Json(json!(a))),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn patch_agent(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<PatchAgentRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE agents SET preferred_model = COALESCE($1, preferred_model), specialty = COALESCE($2, specialty), system_prompt = COALESCE($3, system_prompt) WHERE id = $4")
        .bind(&p.preferred_model).bind(&p.specialty).bind(&p.system_prompt).bind(uid)
        .execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"updated"})))
}

// ═══════════════════════════════════════════════════════════════
// Hiring (CEO / Manager / Worker) — wired to policy engine
// ═══════════════════════════════════════════════════════════════

async fn hire_ceo(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let company_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Count existing CEOs
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM company_ceos WHERE company_id = $1")
        .bind(company_id).fetch_one(&state.db).await.unwrap_or((0,));

    if count.0 >= 2 {
        return (StatusCode::CONFLICT, Json(json!({"error":"Maximum 2 CEOs per company"})));
    }

    // If adding 2nd CEO, create approval request
    if count.0 == 1 {
        let req_id = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO requests (id, type, company_id, payload, status, current_approver_type) VALUES ($1,'ADD_SECOND_CEO',$2,$3,'PENDING','USER')")
            .bind(req_id).bind(company_id).bind(json!({"name": payload.name, "specialty": payload.specialty}))
            .execute(&state.db).await;
        let _ = state.tx.send(json!({"type":"approval_required","request_id": req_id, "request_type":"ADD_SECOND_CEO"}).to_string());
        return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id})));
    }

    // Create CEO agent directly
    let holding: Option<Holding> = sqlx::query_as("SELECT id, owner_user_id, name, main_agent_name, created_at FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let holding_id = holding.map(|h| h.id).unwrap_or(Uuid::from_u128(0));
    let ceo_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'ceo_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = ceo_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let model = payload.preferred_model.unwrap_or_else(|| "glm-5:cloud".into());
    let agent_id = Uuid::new_v4();

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'CEO',$4,$5,$6,$7,'ACTIVE')"
    ).bind(agent_id).bind(holding_id).bind(company_id).bind(&payload.name).bind(&payload.specialty).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    let _ = sqlx::query("INSERT INTO company_ceos (company_id, agent_id) VALUES ($1, $2)")
        .bind(company_id).bind(agent_id).execute(&state.db).await;

    let _ = state.tx.send(json!({"type":"ceo_hired","agent_id": agent_id,"company_id": company_id}).to_string());
    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_manager(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let ceo_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let ceo: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'CEO'"
    ).bind(ceo_id).fetch_optional(&state.db).await.unwrap_or(None);
    let ceo = match ceo { Some(c) => c, None => return (StatusCode::NOT_FOUND, Json(json!({"error":"CEO not found"}))) };
    let company_id = match ceo.company_id { Some(c) => c, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"CEO has no company"}))) };

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents WHERE company_id = $1 AND role = 'MANAGER'")
        .bind(company_id).fetch_one(&state.db).await.unwrap_or((0,));
    let new_count = (count.0 + 1) as u32;

    use crate::policy::engine::{can_hire_manager, Role, Decision};
    match can_hire_manager(new_count, Role::Ceo) {
        Decision::AllowedImmediate => {},
        Decision::RequiresRequest { request_type, approver_chain } => {
            let approver = format!("{:?}", approver_chain.first().unwrap_or(&crate::policy::engine::ApproverType::User));
            let req_id = Uuid::new_v4();
            let _ = sqlx::query("INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type) VALUES ($1,$2,$3,$4,$5,'PENDING',$6)")
                .bind(req_id).bind(&request_type).bind(ceo_id).bind(company_id)
                .bind(json!({"name": payload.name, "count": new_count})).bind(&approver)
                .execute(&state.db).await;
            return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id,"approver": approver})));
        },
        Decision::Denied(reason) => return (StatusCode::FORBIDDEN, Json(json!({"error": reason}))),
    }

    let mgr_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'manager_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = mgr_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let model = payload.preferred_model.unwrap_or_else(|| ceo.effective_model.clone());
    let agent_id = Uuid::new_v4();

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'MANAGER',$4,$5,$6,$7,$8,'ACTIVE')"
    ).bind(agent_id).bind(ceo.holding_id).bind(company_id).bind(&payload.name).bind(&payload.specialty).bind(ceo_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_worker(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let mgr_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let mgr: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'MANAGER'"
    ).bind(mgr_id).fetch_optional(&state.db).await.unwrap_or(None);
    let mgr = match mgr { Some(m) => m, None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Manager not found"}))) };
    let company_id = match mgr.company_id { Some(c) => c, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Manager has no company"}))) };

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents WHERE parent_agent_id = $1 AND role = 'WORKER'")
        .bind(mgr_id).fetch_one(&state.db).await.unwrap_or((0,));
    let new_count = (count.0 + 1) as u32;

    use crate::policy::engine::{can_hire_worker, Role, Decision};
    match can_hire_worker(new_count, Role::Manager) {
        Decision::AllowedImmediate => {},
        Decision::RequiresRequest { request_type, approver_chain } => {
            let approver = format!("{:?}", approver_chain.first().unwrap_or(&crate::policy::engine::ApproverType::User));
            let req_id = Uuid::new_v4();
            let _ = sqlx::query("INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type) VALUES ($1,$2,$3,$4,$5,'PENDING',$6)")
                .bind(req_id).bind(&request_type).bind(mgr_id).bind(company_id)
                .bind(json!({"name": payload.name, "count": new_count})).bind(&approver)
                .execute(&state.db).await;
            return (StatusCode::ACCEPTED, Json(json!({"status":"requires_approval","request_id": req_id})));
        },
        Decision::Denied(reason) => return (StatusCode::FORBIDDEN, Json(json!({"error": reason}))),
    }

    let wkr_policy: Option<ToolPolicy> = sqlx::query_as("SELECT id, name, allowlist, denylist, notes FROM tool_policies WHERE name = 'worker_policy' LIMIT 1")
        .fetch_optional(&state.db).await.unwrap_or(None);
    let policy_id = wkr_policy.map(|p| p.id).unwrap_or(Uuid::new_v4());
    let model = payload.preferred_model.unwrap_or_else(|| mgr.effective_model.clone());
    let agent_id = Uuid::new_v4();

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'WORKER',$4,$5,$6,$7,$8,'ACTIVE')"
    ).bind(agent_id).bind(mgr.holding_id).bind(company_id).bind(&payload.name).bind(&payload.specialty).bind(mgr_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

// ═══════════════════════════════════════════════════════════════
// VM Actions
// ═══════════════════════════════════════════════════════════════

async fn vm_start(Path(id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status":"vm_start_initiated","agent_id": id})))
}

async fn vm_stop(Path(id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status":"vm_stop_initiated","agent_id": id})))
}

async fn vm_rebuild(Path(id): Path<String>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"status":"vm_rebuild_initiated","agent_id": id})))
}

async fn agent_panic(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE agents SET status = 'QUARANTINED' WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"agent_quarantined","agent_id": uid}).to_string());
    (StatusCode::OK, Json(json!({"status":"quarantined","agent_id": uid})))
}

// ═══════════════════════════════════════════════════════════════
// Threads & Messages
// ═══════════════════════════════════════════════════════════════

async fn get_threads(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Thread>("SELECT id, type, title, created_by_user_id, created_at FROM threads ORDER BY created_at DESC")
        .fetch_all(&state.db).await {
        Ok(t) => (StatusCode::OK, Json(json!(t))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn get_thread(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Thread>("SELECT id, type, title, created_by_user_id, created_at FROM threads WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await {
        Ok(Some(t)) => (StatusCode::OK, Json(json!(t))),
        _ => (StatusCode::NOT_FOUND, Json(json!({"error":"Thread not found"})))
    }
}

async fn create_thread(State(state): State<AppState>, Json(p): Json<CreateThreadRequest>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, $2, $3)")
        .bind(id).bind(&p.r#type).bind(&p.title).execute(&state.db).await;
    (StatusCode::CREATED, Json(json!({"id": id, "type": p.r#type, "title": p.title})))
}

async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Message>("SELECT id, thread_id, sender_type, sender_id, content, created_at FROM messages WHERE thread_id = $1 ORDER BY created_at ASC")
        .bind(uid).fetch_all(&state.db).await {
        Ok(m) => (StatusCode::OK, Json(json!(m))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn send_message(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateMessageRequest>
) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let msg_id = Uuid::new_v4();
    let sender_type = p.sender_type.unwrap_or_else(|| "USER".into());
    let sender_id = p.sender_id.unwrap_or_else(Uuid::new_v4);

    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content) VALUES ($1,$2,$3,$4,$5) \
         RETURNING id, thread_id, sender_type, sender_id, content, created_at"
    ).bind(msg_id).bind(thread_id).bind(&sender_type).bind(sender_id).bind(&p.content)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());

            // If the sender is a USER, trigger MainAgent to respond
            if sender_type == "USER" {
                let user_text = p.content.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if !user_text.is_empty() {
                    let state_clone = state.clone();
                    let tid = thread_id;
                    tokio::spawn(async move {
                        tracing::info!("MainAgent processing message: '{}'", &user_text[..user_text.len().min(100)]);

                        // Find which agent is a member of this thread (for DMs)
                        let thread_agent_id: Option<Uuid> = sqlx::query_scalar(
                            "SELECT member_id FROM thread_members WHERE thread_id = $1 AND member_type = 'AGENT' LIMIT 1"
                        )
                        .fetch_optional(&state_clone.db)
                        .await
                        .ok()
                        .flatten();

                        // Fall back to MainAgent if no specific agent is on this thread
                        let responding_agent_id: Uuid = match thread_agent_id {
                            Some(id) => id,
                            None => {
                                sqlx::query_scalar(
                                    "SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1"
                                )
                                .fetch_optional(&state_clone.db)
                                .await
                                .ok()
                                .flatten()
                                .unwrap_or(Uuid::new_v4())
                            }
                        };

                        match state_clone.main_agent.handle_message(&state_clone.db, &user_text).await {
                            Ok(response) => {
                                let agent_id = responding_agent_id;

                                // Insert agent response as a new message
                                let resp_id = Uuid::new_v4();
                                let content = json!({"text": response});
                                if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                                    "INSERT INTO messages (id, thread_id, sender_type, sender_id, content) VALUES ($1,$2,'AGENT',$3,$4) \
                                     RETURNING id, thread_id, sender_type, sender_id, content, created_at"
                                )
                                .bind(resp_id)
                                .bind(tid)
                                .bind(agent_id)
                                .bind(&content)
                                .fetch_one(&state_clone.db)
                                .await {
                                    let _ = state_clone.tx.send(
                                        json!({"type":"new_message","message": agent_msg}).to_string()
                                    );
                                    tracing::info!("MainAgent responded on thread {}", tid);
                                }
                            }
                            Err(e) => {
                                tracing::error!("MainAgent error: {}", e);
                                // Insert error message so user sees something
                                let resp_id = Uuid::new_v4();
                                let content = json!({"text": format!("Sorry, I encountered an error: {}", e)});
                                let _ = sqlx::query(
                                    "INSERT INTO messages (id, thread_id, sender_type, sender_id, content) VALUES ($1,$2,'AGENT',$3,$4)"
                                )
                                .bind(resp_id)
                                .bind(tid)
                                .bind(Uuid::new_v4())
                                .bind(&content)
                                .execute(&state_clone.db)
                                .await;
                            }
                        }
                    });
                }
            }

            (StatusCode::CREATED, Json(json!(msg)))
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Requests & Approvals
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct RequestQuery { status: Option<String> }

async fn list_requests(State(state): State<AppState>, Query(q): Query<RequestQuery>) -> impl IntoResponse {
    let status_filter = q.status.unwrap_or_else(|| "%".into());
    match sqlx::query_as::<_, Request>(
        "SELECT id, type, created_by_agent_id, created_by_user_id, company_id, payload, status, current_approver_type, current_approver_id, created_at, updated_at \
         FROM requests WHERE status LIKE $1 ORDER BY created_at DESC"
    ).bind(&status_filter).fetch_all(&state.db).await {
        Ok(r) => (StatusCode::OK, Json(json!(r))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn create_request(State(state): State<AppState>, Json(p): Json<CreateRequestPayload>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO requests (id, type, company_id, payload, status, current_approver_type) VALUES ($1,$2,$3,$4,'PENDING','USER')"
    ).bind(id).bind(&p.r#type).bind(p.company_id).bind(&p.payload).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"new_request","request_id": id}).to_string());
    (StatusCode::CREATED, Json(json!({"id": id, "status":"PENDING"})))
}

async fn approve_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'APPROVE',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"request_approved","request_id": uid}).to_string());
    (StatusCode::OK, Json(json!({"status":"approved"})))
}

async fn reject_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'REJECT',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'REJECTED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"rejected"})))
}

// ═══════════════════════════════════════════════════════════════
// Services & Engagements
// ═══════════════════════════════════════════════════════════════

async fn list_services(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, ServiceCatalogItem>(
        "SELECT id, provider_company_id, name, description, pricing_model, rate, tags, active, created_at FROM service_catalog WHERE active = true"
    ).fetch_all(&state.db).await {
        Ok(s) => (StatusCode::OK, Json(json!(s))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

async fn create_service(State(state): State<AppState>, Json(p): Json<CreateServiceRequest>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO service_catalog (id, provider_company_id, name, description, pricing_model, rate) VALUES ($1,$2,$3,$4,$5,$6)"
    ).bind(id).bind(p.provider_company_id).bind(&p.name).bind(&p.description).bind(&p.pricing_model).bind(&p.rate)
    .execute(&state.db).await;
    (StatusCode::CREATED, Json(json!({"id": id})))
}

async fn create_engagement(State(state): State<AppState>, Json(p): Json<CreateEngagementRequest>) -> impl IntoResponse {
    let thread_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'ENGAGEMENT', 'Service Engagement')")
        .bind(thread_id).execute(&state.db).await;

    let svc: Option<ServiceCatalogItem> = sqlx::query_as(
        "SELECT id, provider_company_id, name, description, pricing_model, rate, tags, active, created_at FROM service_catalog WHERE id = $1"
    ).bind(p.service_id).fetch_optional(&state.db).await.unwrap_or(None);
    let provider_id = svc.map(|s| s.provider_company_id).unwrap_or(Uuid::new_v4());

    let id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO service_engagements (id, service_id, client_company_id, provider_company_id, scope, status, created_by_agent_id, thread_id) VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
    ).bind(id).bind(p.service_id).bind(p.client_company_id).bind(provider_id).bind(&p.scope).bind(p.created_by_agent_id).bind(thread_id)
    .execute(&state.db).await;
    (StatusCode::CREATED, Json(json!({"id": id, "thread_id": thread_id})))
}

async fn activate_engagement(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE service_engagements SET status = 'ACTIVE' WHERE id = $1").bind(uid).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"activated"})))
}

async fn complete_engagement(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE service_engagements SET status = 'COMPLETED' WHERE id = $1").bind(uid).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"completed"})))
}

// ═══════════════════════════════════════════════════════════════
// Ledger
// ═══════════════════════════════════════════════════════════════

async fn get_ledger(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, LedgerEntry>(
        "SELECT id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual, created_at \
         FROM ledger_entries WHERE company_id = $1 ORDER BY created_at DESC"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(l) => (StatusCode::OK, Json(json!(l))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

// ═══════════════════════════════════════════════════════════════
// Agentd Registration (called by VMs)
// ═══════════════════════════════════════════════════════════════

async fn agentd_register() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"registered"})))
}

async fn agentd_heartbeat() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"ok"})))
}

// ═══════════════════════════════════════════════════════════════
// Update Company
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct UpdateCompanyRequest {
    name: Option<String>,
    r#type: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

async fn update_company(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<UpdateCompanyRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query(
        "UPDATE companies SET name = COALESCE($1, name), type = COALESCE($2, type), description = COALESCE($3, description), status = COALESCE($4, status) WHERE id = $5"
    )
    .bind(&p.name).bind(&p.r#type).bind(&p.description).bind(&p.status).bind(uid)
    .execute(&state.db).await;
    
    // Re-fetch and return
    match sqlx::query_as::<_, Company>(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await {
        Ok(Some(c)) => {
            let _ = state.tx.send(json!({"type":"company_updated","company": c}).to_string());
            (StatusCode::OK, Json(json!(c)))
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({"error":"Company not found"})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent DM Thread (get or create)
// ═══════════════════════════════════════════════════════════════

async fn get_agent_thread(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Check if a DM thread already exists with this agent
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT tm.thread_id FROM thread_members tm JOIN threads t ON t.id = tm.thread_id \
         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 AND t.type = 'DM' LIMIT 1"
    ).bind(agent_id).fetch_optional(&state.db).await.unwrap_or(None);

    if let Some((thread_id,)) = existing {
        return (StatusCode::OK, Json(json!({"thread_id": thread_id, "created": false})));
    }

    // Get agent info for thread title
    let agent_name: Option<String> = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(&state.db).await.ok().flatten();
    let name = agent_name.unwrap_or_else(|| "Agent".into());

    // Create new DM thread
    let thread_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
        .bind(thread_id).bind(format!("DM with {}", name))
        .execute(&state.db).await;

    // Add agent as member
    let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
        .bind(thread_id).bind(agent_id).execute(&state.db).await;

    // Add USER as member (placeholder user ID)
    let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'USER', $2)")
        .bind(thread_id).bind(Uuid::from_u128(0)).execute(&state.db).await;

    (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "created": true})))
}

// ═══════════════════════════════════════════════════════════════
// Thread Participants
// ═══════════════════════════════════════════════════════════════

#[derive(sqlx::FromRow, serde::Serialize)]
struct ThreadMember {
    thread_id: Uuid,
    member_type: String,
    member_id: Uuid,
}

async fn get_thread_participants(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, ThreadMember>(
        "SELECT thread_id, member_type, member_id FROM thread_members WHERE thread_id = $1"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(members) => (StatusCode::OK, Json(json!(members))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

// ═══════════════════════════════════════════════════════════════
// System Update Check & Update
// ═══════════════════════════════════════════════════════════════

const CURRENT_VERSION: &str = "0.1.1";

async fn system_update_check(State(_state): State<AppState>) -> impl IntoResponse {
    // Check GitHub for latest release
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let github_url = "https://api.github.com/repos/8PotatoChip8/MultiClaw/releases/latest";
    
    match client.get(github_url)
        .header("User-Agent", "MultiClaw-Updater")
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<Value>().await {
                    let latest = body["tag_name"].as_str().unwrap_or("unknown").trim_start_matches('v');
                    let update_available = latest != CURRENT_VERSION && latest != "unknown";
                    return (StatusCode::OK, Json(json!({
                        "current_version": CURRENT_VERSION,
                        "latest_version": latest,
                        "update_available": update_available,
                        "release_url": body["html_url"].as_str().unwrap_or("")
                    })));
                }
            }
            // No releases yet — that's fine
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": CURRENT_VERSION,
                "update_available": false,
                "release_url": ""
            })))
        }
        Err(_) => {
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": "unknown",
                "update_available": false,
                "error": "Could not reach GitHub"
            })))
        }
    }
}

async fn system_update(State(state): State<AppState>) -> impl IntoResponse {
    let state_clone = state.clone();
    tokio::spawn(async move {
        tracing::info!("Starting system update...");
        let _ = state_clone.tx.send(json!({"type":"system_update","status":"started"}).to_string());

        // Pull latest code
        let pull = tokio::process::Command::new("git")
            .args(["-C", "/opt/multiclaw", "pull", "origin", "main"])
            .output()
            .await;

        match pull {
            Ok(output) => {
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git pull failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                tracing::info!("Git pull successful, rebuilding containers...");
            }
            Err(e) => {
                tracing::error!("Git pull error: {}", e);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                return;
            }
        }

        // Rebuild and restart containers
        let rebuild = tokio::process::Command::new("docker")
            .args(["compose", "-f", "/opt/multiclaw/infra/docker/docker-compose.yml", "up", "-d", "--build"])
            .output()
            .await;

        match rebuild {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!("System update complete!");
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"complete"}).to_string());
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Docker rebuild failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                }
            }
            Err(e) => {
                tracing::error!("Docker rebuild error: {}", e);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
            }
        }
    });

    (StatusCode::ACCEPTED, Json(json!({"status":"update_started"})))
}
