use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, patch, put, delete},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
use tower_http::cors::{CorsLayer, Any};

use super::ws::{events_handler, AppState};
use crate::db::models::*;
use crate::provisioning::vm_provider::{VmProvider, VmResources};
use crate::provisioning::cloudinit::{CloudInitArgs, render_cloud_init};

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
        .route("/v1/agents/:id/vm/provision", post(vm_provision))
        .route("/v1/agents/:id/vm/sandbox/provision", post(vm_sandbox_provision))
        .route("/v1/agents/:id/vm/exec", post(vm_exec))
        .route("/v1/agents/:id/vm/info", get(vm_info))
        .route("/v1/agents/:id/vm/file/push", post(vm_file_push))
        .route("/v1/agents/:id/vm/file/pull", post(vm_file_pull))
        .route("/v1/agents/:id/panic", post(agent_panic))
        .route("/v1/agents/:id/thread", get(get_agent_thread))
        .route("/v1/agents/:id/memories", get(get_agent_memories).post(create_agent_memory))
        .route("/v1/agents/:id/memories/:mid", delete(delete_agent_memory))
        .route("/v1/agents/:id/openclaw-files", get(get_openclaw_files))
        // Threads & Messages
        .route("/v1/threads", get(get_threads).post(create_thread))
        .route("/v1/threads/:id", get(get_thread))
        .route("/v1/threads/:id/messages", get(get_messages).post(send_message))
        .route("/v1/threads/:id/participants", get(get_thread_participants).post(add_thread_participant))
        .route("/v1/threads/:id/participants/:member_id", delete(remove_thread_participant))
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
        // Scripts (served to VMs during cloud-init)
        .route("/v1/scripts/install-openclaw.sh", get(serve_install_script))
        // System
        .route("/v1/system/settings", get(get_system_settings))
        .route("/v1/system/settings", put(update_system_settings))
        .route("/v1/system/update-check", get(system_update_check))
        .route("/v1/system/update", post(system_update))
        .route("/v1/system/containers", get(list_containers))
        .route("/v1/system/containers/:id/logs", get(get_container_logs))
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
    body: axum::body::Bytes,
) -> impl IntoResponse {
    tracing::info!("Received init request, body length={}", body.len());

    let payload: InitRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to parse init request JSON: {}. Body: {:?}", e, String::from_utf8_lossy(&body));
            return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid JSON: {}", e)})));
        }
    };

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

    // Spawn OpenClaw instance for MainAgent in background
    let openclaw = state.openclaw.clone();
    let agent_name_clone = agent_name.clone();
    let model_clone = model.clone();
    let holding_name_clone = holding_name.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id,
            agent_name: agent_name_clone,
            role: "MAIN".to_string(),
            company_name: holding_name_clone.clone(),
            holding_name: holding_name_clone,
            specialty: Some("Holding Company Management".to_string()),
            model: model_clone,
            system_prompt: None,
        };
        match openclaw.spawn_instance(&config).await {
            Ok(inst) => tracing::info!("OpenClaw instance spawned for MainAgent on port {}", inst.port),
            Err(e) => tracing::error!("Failed to spawn OpenClaw instance for MainAgent: {}", e),
        }
    });

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

    // Check for duplicate company name
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM companies WHERE holding_id = $1 AND LOWER(name) = LOWER($2)"
    ).bind(holding_id).bind(&payload.name).fetch_optional(&state.db).await.unwrap_or(None);

    if let Some((existing_id,)) = existing {
        return (StatusCode::CONFLICT, Json(json!({
            "error": format!("A company named '{}' already exists", payload.name),
            "existing_id": existing_id
        })));
    }

    match sqlx::query_as::<_, Company>(
        "INSERT INTO companies (id, holding_id, name, type, description, status) VALUES ($1,$2,$3,$4,$5,'ACTIVE') \
         RETURNING id, holding_id, name, type, description, tags, status, created_at"
    ).bind(id).bind(holding_id).bind(&payload.name).bind(&payload.r#type).bind(&payload.description)
    .fetch_one(&state.db).await {
        Ok(c) => {
            let _ = state.tx.send(json!({"type":"company_created","company": c}).to_string());
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
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
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
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents ORDER BY created_at"
    ).fetch_all(&state.db).await {
        Ok(a) => (StatusCode::OK, Json(json!(a))),
        Err(e) => { tracing::error!("list_agents: {}", e); (StatusCode::OK, Json(json!([]))) }
    }
}

async fn get_agent(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Agent>(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
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

    // If adding 2nd CEO, require explicit approval
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
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'CEO',$4,$5,$6,$7,$8,'ACTIVE')"
    ).bind(agent_id).bind(holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    let _ = sqlx::query("INSERT INTO company_ceos (company_id, agent_id) VALUES ($1, $2)")
        .bind(company_id).bind(agent_id).execute(&state.db).await;

    // Spawn OpenClaw instance in background
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let company_name: String = sqlx::query_scalar("SELECT name FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Company".into());
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "CEO".to_string(),
            company_name, holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for CEO {}: {}", config.agent_name, e);
        }
    });

    let _ = state.tx.send(json!({"type":"ceo_hired","agent_id": agent_id,"company_id": company_id}).to_string());
    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_manager(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let ceo_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let ceo: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'CEO'"
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
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'MANAGER',$4,$5,$6,$7,$8,$9,'ACTIVE')"
    ).bind(agent_id).bind(ceo.holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(ceo_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    // Spawn OpenClaw instance in background
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let company_name: String = sqlx::query_scalar("SELECT name FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Company".into());
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "MANAGER".to_string(),
            company_name, holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for Manager {}: {}", config.agent_name, e);
        }
    });

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

async fn hire_worker(State(state): State<AppState>, Path(id): Path<String>, Json(payload): Json<HireRequest>) -> impl IntoResponse {
    let mgr_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let mgr: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1 AND role = 'MANAGER'"
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
    let handle = format!("@{}", payload.name.to_lowercase().replace(' ', "-"));

    let _ = sqlx::query(
        "INSERT INTO agents (id, holding_id, company_id, role, name, handle, specialty, parent_agent_id, effective_model, tool_policy_id, status) VALUES ($1,$2,$3,'WORKER',$4,$5,$6,$7,$8,$9,'ACTIVE')"
    ).bind(agent_id).bind(mgr.holding_id).bind(company_id).bind(&payload.name).bind(&handle).bind(&payload.specialty).bind(mgr_id).bind(&model).bind(policy_id)
    .execute(&state.db).await;

    // Spawn OpenClaw instance in background
    let openclaw = state.openclaw.clone();
    let name_clone = payload.name.clone();
    let company_name: String = sqlx::query_scalar("SELECT name FROM companies WHERE id = $1").bind(company_id)
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Company".into());
    let holding_name: String = sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Holdings".into());
    let model_clone = model.clone();
    let specialty_clone = payload.specialty.clone();
    tokio::spawn(async move {
        let config = crate::openclaw::AgentConfig {
            agent_id, agent_name: name_clone, role: "WORKER".to_string(),
            company_name, holding_name, specialty: specialty_clone,
            model: model_clone, system_prompt: None,
        };
        if let Err(e) = openclaw.spawn_instance(&config).await {
            tracing::error!("Failed to spawn OpenClaw for Worker {}: {}", config.agent_name, e);
        }
    });

    (StatusCode::CREATED, Json(json!({"status":"hired","agent_id": agent_id})))
}

// ═══════════════════════════════════════════════════════════════
// VM Provisioning Helper
// ═══════════════════════════════════════════════════════════════

/// Provisions an Incus VM for the given agent in a background task.
/// Reads cloud-init templates, renders config, launches the VM,
/// and updates the agent's vm_id in the database.
async fn provision_agent_vm(
    state: AppState,
    agent_id: Uuid,
    agent_name: &str,
    model: &str,
    policy_name: &str,
) {
    let provider = match state.vm_provider {
        Some(ref p) => p.clone(),
        None => {
            tracing::warn!("VM provider not available, skipping VM provisioning for agent {}", agent_id);
            return;
        }
    };

    let host_ip = state.config.host_ip.clone();
    let agent_name = agent_name.to_string();
    let model = model.to_string();
    let policy_name = policy_name.to_string();
    let db = state.db.clone();
    let tx = state.tx.clone();

    tokio::spawn(async move {
        tracing::info!("Provisioning VM for agent {} ({})", agent_name, agent_id);

        // Load tool policy
        let (tools_allow, tools_deny) = {
            let policy: Option<(Value, Value)> = sqlx::query_as(
                "SELECT allowlist, denylist FROM tool_policies WHERE name = $1 LIMIT 1"
            ).bind(&policy_name).fetch_optional(&db).await.unwrap_or(None);
            match policy {
                Some((a, d)) => (a.to_string(), d.to_string()),
                None => ("[\"*\"]".to_string(), "[]".to_string()),
            }
        };

        // Generate tokens for this agent
        let gateway_token = Uuid::new_v4().to_string();
        let agent_token = Uuid::new_v4().to_string();
        let ollama_token = Uuid::new_v4().to_string();

        // Load templates (embedded as compile-time constants would be ideal,
        // but for now read from /opt/multiclaw or the infra/vm directory)
        let base_paths = ["/opt/multiclaw/infra/vm", "infra/vm"];
        let mut base = "";
        for p in &base_paths {
            if std::path::Path::new(p).exists() {
                base = p;
                break;
            }
        }

        if base.is_empty() {
            tracing::error!("VM templates not found, skipping VM provisioning for agent {}", agent_id);
            return;
        }

        let tmpl_user_data = match tokio::fs::read_to_string(format!("{}/cloud-init/agent-user-data.yaml.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read cloud-init template: {}", e); return; }
        };
        let tmpl_openclaw_json = match tokio::fs::read_to_string(format!("{}/openclaw/openclaw.json.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read openclaw.json template: {}", e); return; }
        };
        let tmpl_openclaw_svc = match tokio::fs::read_to_string(format!("{}/systemd/openclaw-gateway.service.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read openclaw service template: {}", e); return; }
        };
        let tmpl_agentd_svc = match tokio::fs::read_to_string(format!("{}/systemd/multiclaw-agentd.service.tmpl", base)).await {
            Ok(t) => t,
            Err(e) => { tracing::error!("Failed to read agentd service template: {}", e); return; }
        };

        let vm_name = format!("mc-{}", &agent_id.to_string()[..8]);

        let args = CloudInitArgs {
            hostname: vm_name.clone(),
            host_ip: host_ip.clone(),
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            effective_model: model.clone(),
            agent_token,
            openclaw_gateway_token: gateway_token,
            ollama_token,
            tools_allow,
            tools_deny,
            tmpl_user_data,
            tmpl_openclaw_json,
            tmpl_openclaw_svc,
            tmpl_agentd_svc,
        };

        let cloud_init = match render_cloud_init(&args) {
            Ok(ci) => ci,
            Err(e) => { tracing::error!("Failed to render cloud-init: {}", e); return; }
        };

        let resources = VmResources {
            vcpus: 2,
            memory_mb: 2048,
            disk_gb: 20,
        };

        match provider.provision(&vm_name, &resources, &cloud_init).await {
            Ok(details) => {
                tracing::info!("VM '{}' provisioned for agent {}, ip={:?}", vm_name, agent_id, details.ip_address);
                // Insert record into vms table
                let vm_uuid = Uuid::new_v4();
                let resources_json = serde_json::json!({
                    "vcpus": resources.vcpus,
                    "memory_mb": resources.memory_mb,
                    "disk_gb": resources.disk_gb
                });
                let _ = sqlx::query(
                    "INSERT INTO vms (id, provider, provider_ref, hostname, ip_address, resources, state) \
                     VALUES ($1, 'incus', $2, $3, $4, $5, 'RUNNING')"
                )
                .bind(vm_uuid)
                .bind(&vm_name)
                .bind(&vm_name)
                .bind(&details.ip_address)
                .bind(&resources_json)
                .execute(&db).await;

                // Link agent to vm
                let _ = sqlx::query("UPDATE agents SET vm_id = $1 WHERE id = $2")
                    .bind(vm_uuid).bind(agent_id)
                    .execute(&db).await;

                let _ = tx.send(json!({
                    "type": "vm_provisioned",
                    "agent_id": agent_id,
                    "vm_id": vm_name,
                    "ip": details.ip_address
                }).to_string());
            }
            Err(e) => {
                tracing::error!("Failed to provision VM for agent {}: {}", agent_id, e);
                let _ = tx.send(json!({
                    "type": "vm_provision_failed",
                    "agent_id": agent_id,
                    "error": e.to_string()
                }).to_string());
            }
        }
    });
}

// ═══════════════════════════════════════════════════════════════
// VM Target Resolution
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmTargetQuery {
    target: Option<String>, // "desktop" (default) or "sandbox"
}

async fn resolve_vm_ref(db: &sqlx::PgPool, agent_id: Uuid, target: &str) -> Option<String> {
    if target == "sandbox" {
        sqlx::query_scalar(
            "SELECT v.provider_ref FROM vms v JOIN agents a ON a.sandbox_vm_id = v.id WHERE a.id = $1"
        ).bind(agent_id).fetch_optional(db).await.ok().flatten()
    } else {
        sqlx::query_scalar(
            "SELECT v.provider_ref FROM vms v JOIN agents a ON a.vm_id = v.id WHERE a.id = $1"
        ).bind(agent_id).fetch_optional(db).await.ok().flatten()
    }
}

// ═══════════════════════════════════════════════════════════════
// VM Actions
// ═══════════════════════════════════════════════════════════════

async fn vm_start(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if state.vm_provider.is_some() {
                let _ = tokio::process::Command::new("incus").args(&["start", name]).output().await;
                (StatusCode::ACCEPTED, Json(json!({"status":"vm_started","vm_name": name, "target": target})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_stop(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let _ = provider.stop(name).await;
                (StatusCode::ACCEPTED, Json(json!({"status":"vm_stopped","vm_name": name, "target": target})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_rebuild(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");

    // Cannot wipe/rebuild the persistent desktop VM
    if target == "desktop" {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Cannot wipe the persistent desktop VM. Only sandbox VMs can be rebuilt."})));
    }

    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let _ = provider.destroy(name).await;
                // Clean up: remove vms record and unlink sandbox from agent
                let _ = sqlx::query(
                    "DELETE FROM vms WHERE id = (SELECT sandbox_vm_id FROM agents WHERE id = $1)"
                ).bind(uid).execute(&state.db).await;
                let _ = sqlx::query("UPDATE agents SET sandbox_vm_id = NULL WHERE id = $1")
                    .bind(uid).execute(&state.db).await;
                (StatusCode::ACCEPTED, Json(json!({"status":"sandbox_destroyed_for_rebuild","vm_name": name})))
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No sandbox VM assigned to this agent"})))
    }
}

/// Provision a VM on-demand for an agent (their "workstation")
async fn vm_provision(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Check if agent already has a VM
    let existing_vm: Option<Uuid> = sqlx::query_scalar(
        "SELECT vm_id FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if existing_vm.is_some() {
        return (StatusCode::CONFLICT, Json(json!({"error": "Agent already has a VM assigned", "vm_id": existing_vm})));
    }

    // Get agent info for provisioning
    let agent_info: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT name, effective_model, (SELECT name FROM tool_policies WHERE id = a.tool_policy_id) FROM agents a WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    match agent_info {
        Some((name, model, policy_name)) => {
            let policy = policy_name.unwrap_or_else(|| "worker_policy".into());
            provision_agent_vm(state.clone(), uid, &name, &model, &policy).await;
            (StatusCode::ACCEPTED, Json(json!({"status": "provisioning", "agent_id": uid, "message": format!("VM provisioning started for {}", name)})))
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "Agent not found"})))
    }
}

/// Provision a sandbox VM for an agent (lightweight temp environment)
async fn vm_sandbox_provision(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Check if agent already has a sandbox VM
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT sandbox_vm_id FROM agents WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if existing.is_some() {
        return (StatusCode::CONFLICT, Json(json!({"error": "Agent already has a sandbox VM assigned"})));
    }

    let provider = match state.vm_provider {
        Some(ref p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"}))),
    };

    let db = state.db.clone();
    let tx = state.tx.clone();

    tokio::spawn(async move {
        let vm_name = format!("mc-{}-sb", &uid.to_string()[..8]);
        tracing::info!("Provisioning sandbox VM '{}' for agent {}", vm_name, uid);

        // Minimal cloud-init: just create agent user
        let cloud_init = format!(
            "#cloud-config\nhostname: {}\nusers:\n  - name: agent\n    shell: /bin/bash\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    groups: sudo\npackage_update: true\npackages:\n  - curl\n  - git\n  - build-essential\n",
            vm_name
        );

        let resources = VmResources { vcpus: 1, memory_mb: 1024, disk_gb: 10 };

        match provider.provision(&vm_name, &resources, &cloud_init).await {
            Ok(details) => {
                tracing::info!("Sandbox VM '{}' provisioned for agent {}, ip={:?}", vm_name, uid, details.ip_address);
                let vm_uuid = Uuid::new_v4();
                let resources_json = serde_json::json!({
                    "vcpus": resources.vcpus, "memory_mb": resources.memory_mb, "disk_gb": resources.disk_gb
                });
                let _ = sqlx::query(
                    "INSERT INTO vms (id, provider, provider_ref, hostname, ip_address, resources, state, vm_type) \
                     VALUES ($1, 'incus', $2, $3, $4, $5, 'RUNNING', 'sandbox')"
                )
                .bind(vm_uuid).bind(&vm_name).bind(&vm_name)
                .bind(&details.ip_address).bind(&resources_json)
                .execute(&db).await;

                let _ = sqlx::query("UPDATE agents SET sandbox_vm_id = $1 WHERE id = $2")
                    .bind(vm_uuid).bind(uid).execute(&db).await;

                let _ = tx.send(json!({
                    "type": "sandbox_provisioned", "agent_id": uid,
                    "vm_id": vm_name, "ip": details.ip_address
                }).to_string());
            }
            Err(e) => {
                tracing::error!("Failed to provision sandbox VM for agent {}: {}", uid, e);
                let _ = tx.send(json!({
                    "type": "sandbox_provision_failed", "agent_id": uid, "error": e.to_string()
                }).to_string());
            }
        }
    });

    (StatusCode::ACCEPTED, Json(json!({"status": "provisioning_sandbox", "agent_id": uid})))
}

async fn agent_panic(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let _ = sqlx::query("UPDATE agents SET status = 'QUARANTINED' WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"agent_quarantined","agent_id": uid}).to_string());
    (StatusCode::OK, Json(json!({"status":"quarantined","agent_id": uid})))
}

// ═══════════════════════════════════════════════════════════════
// VM Interaction: Exec, Info, File Push/Pull
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmExecRequest {
    command: String,
    user: Option<String>,
    working_dir: Option<String>,
    timeout_secs: Option<u64>,
}

async fn vm_exec(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmExecRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.exec_command(
                    name,
                    &body.command,
                    body.user.as_deref().or(Some("agent")),
                    body.working_dir.as_deref().or(Some("/home/agent")),
                    body.timeout_secs,
                ).await {
                    Ok(result) => (StatusCode::OK, Json(json!(result))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

async fn vm_info(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.get_info(name).await {
                    Ok(info) => (StatusCode::OK, Json(json!(info))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

#[derive(Debug, Deserialize)]
struct VmFilePushRequest {
    path: String,
    content: String,
    encoding: Option<String>, // "base64" or "text" (default)
}

async fn vm_file_push(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmFilePushRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                let bytes = if body.encoding.as_deref() == Some("base64") {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(&body.content) {
                        Ok(b) => b,
                        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid base64: {}", e)}))),
                    }
                } else {
                    body.content.into_bytes()
                };
                match provider.file_push(name, &bytes, &body.path).await {
                    Ok(()) => (StatusCode::OK, Json(json!({"status":"ok","path": body.path}))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
}

#[derive(Debug, Deserialize)]
struct VmFilePullRequest {
    path: String,
}

async fn vm_file_pull(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<VmTargetQuery>, Json(body): Json<VmFilePullRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let target = q.target.as_deref().unwrap_or("desktop");
    let vm_ref = resolve_vm_ref(&state.db, uid, target).await;
    match vm_ref {
        Some(ref name) => {
            if let Some(ref provider) = state.vm_provider {
                match provider.file_pull(name, &body.path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content).to_string();
                        (StatusCode::OK, Json(json!({"path": body.path, "content": text, "size": content.len()})))
                    }
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
                }
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"})))
            }
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"No VM assigned to this agent"})))
    }
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

                        // Check thread type
                        let thread_type: String = sqlx::query_scalar(
                            "SELECT type FROM threads WHERE id = $1"
                        )
                        .bind(tid)
                        .fetch_optional(&state_clone.db)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "DM".to_string());

                        // Get agent members of this thread
                        let agent_ids: Vec<Uuid> = sqlx::query_scalar(
                            "SELECT member_id FROM thread_members WHERE thread_id = $1 AND member_type = 'AGENT'"
                        )
                        .bind(tid)
                        .fetch_all(&state_clone.db)
                        .await
                        .unwrap_or_default();

                        // Determine which agents should respond
                        let responding_agents: Vec<Uuid> = if thread_type == "GROUP" {
                            // Check for @-mentions in the message (format: @agent-name or @handle)
                            let mut mentioned: Vec<Uuid> = Vec::new();
                            for aid in &agent_ids {
                                let agent_info: Option<(String, Option<String>)> = sqlx::query_as(
                                    "SELECT name, handle FROM agents WHERE id = $1"
                                ).bind(aid).fetch_optional(&state_clone.db).await.ok().flatten();
                                if let Some((name, handle)) = agent_info {
                                    let lower_text = user_text.to_lowercase();
                                    if lower_text.contains(&format!("@{}", name.to_lowercase().replace(' ', "-")))
                                        || handle.as_ref().map(|h| lower_text.contains(&h.to_lowercase())).unwrap_or(false)
                                        || lower_text.contains(&name.to_lowercase())
                                    {
                                        mentioned.push(*aid);
                                    }
                                }
                            }
                            if mentioned.is_empty() {
                                // No specific mention — route to ALL agents in the group (max 3)
                                agent_ids.iter().take(3).cloned().collect()
                            } else {
                                // Route only to mentioned agents (max 3)
                                mentioned.into_iter().take(3).collect()
                            }
                        } else {
                            // DM: single agent
                            if let Some(aid) = agent_ids.first() {
                                vec![*aid]
                            } else {
                                // Fallback to MainAgent
                                let main_id: Option<Uuid> = sqlx::query_scalar(
                                    "SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1"
                                )
                                .fetch_optional(&state_clone.db)
                                .await
                                .ok()
                                .flatten();
                                main_id.into_iter().collect()
                            }
                        };

                        // Send to each responding agent (sequentially to avoid token waste)
                        for responding_agent_id in &responding_agents {
                            let result: Result<String, anyhow::Error> = match state_clone.openclaw.send_message(*responding_agent_id, &user_text).await {
                                Ok(response) => {
                                    tracing::info!("OpenClaw responded for agent {}", responding_agent_id);
                                    Ok(response)
                                }
                                Err(e) => {
                                    tracing::warn!("OpenClaw unavailable for agent {}: {}", responding_agent_id, e);
                                    let agent_name: String = sqlx::query_scalar(
                                        "SELECT name FROM agents WHERE id = $1"
                                    )
                                    .bind(responding_agent_id)
                                    .fetch_optional(&state_clone.db)
                                    .await
                                    .ok()
                                    .flatten()
                                    .unwrap_or_else(|| "Agent".to_string());

                                    Ok(format!(
                                        "⚠️ {} is currently unavailable — their OpenClaw runtime is not running. \
                                         Please wait for their instance to come online before sending messages.",
                                        agent_name
                                    ))
                                }
                            };

                            match result {
                                Ok(response) => {
                                    let resp_id = Uuid::new_v4();
                                    let content = json!({"text": response});
                                    if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                                        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content) VALUES ($1,$2,'AGENT',$3,$4) \
                                         RETURNING id, thread_id, sender_type, sender_id, content, created_at"
                                    )
                                    .bind(resp_id)
                                    .bind(tid)
                                    .bind(responding_agent_id)
                                    .bind(&content)
                                    .fetch_one(&state_clone.db)
                                    .await {
                                        let _ = state_clone.tx.send(
                                            json!({"type":"new_message","message": agent_msg}).to_string()
                                        );
                                        tracing::info!("Agent {} responded on thread {}", responding_agent_id, tid);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Agent error: {}", e);
                                    let resp_id = Uuid::new_v4();
                                    let content = json!({"text": format!("Sorry, I encountered an error: {}", e)});
                                    let _ = sqlx::query(
                                        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content) VALUES ($1,$2,'AGENT',$3,$4)"
                                    )
                                    .bind(resp_id)
                                    .bind(tid)
                                    .bind(responding_agent_id)
                                    .bind(&content)
                                    .execute(&state_clone.db)
                                    .await;
                                }
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

// ═══════════════════════════════════════════════════════════════
// Scripts (served to agent VMs during cloud-init)
// ═══════════════════════════════════════════════════════════════

async fn serve_install_script() -> impl IntoResponse {
    let paths = ["/opt/multiclaw/infra/vm/scripts/install-openclaw.sh", "infra/vm/scripts/install-openclaw.sh"];
    for p in &paths {
        if let Ok(content) = tokio::fs::read_to_string(p).await {
            return (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                content,
            ).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "install script not found").into_response()
}

const CURRENT_VERSION: &str = "0.1.1";

async fn system_update_check(State(state): State<AppState>) -> impl IntoResponse {
    // Read update channel from system_meta (default: stable)
    let channel: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'update_channel'")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "stable".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let repo = "8PotatoChip8/MultiClaw";

    match channel.as_str() {
        "beta" | "dev" => {
            // Compare latest commit SHA on the target branch vs deployed commit
            let branch = if channel == "beta" { "beta" } else { "main" };
            let url = format!("https://api.github.com/repos/{}/commits/{}", repo, branch);

            // Get deployed commit from system_meta
            let deployed_commit: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'deployed_commit'")
                .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "unknown".to_string());

            match client.get(&url).header("User-Agent", "MultiClaw-Updater").send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<Value>().await {
                        let latest_sha = body["sha"].as_str().unwrap_or("unknown");
                        let short_sha = &latest_sha[..7.min(latest_sha.len())];
                        let deployed_short = &deployed_commit[..7.min(deployed_commit.len())];
                        let commit_msg = body["commit"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("");
                        let update_available = deployed_commit == "unknown" || latest_sha != deployed_commit;
                        // For dev/beta: always use commit-based format so comparison is consistent
                        let current_display = format!("{}-{}", channel, deployed_short);
                        return (StatusCode::OK, Json(json!({
                            "current_version": current_display,
                            "latest_version": format!("{}-{}", channel, short_sha),
                            "update_available": update_available,
                            "channel": channel,
                            "semver": CURRENT_VERSION,
                            "deployed_commit": deployed_short,
                            "latest_commit": short_sha,
                            "commit_message": commit_msg,
                            "release_url": format!("https://github.com/{}/commit/{}", repo, latest_sha)
                        })));
                    }
                },
                _ => {}
            }
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": "unknown",
                "update_available": false,
                "channel": channel,
                "semver": CURRENT_VERSION,
                "error": format!("Could not reach GitHub (branch: {})", branch)
            })))
        },
        _ => {
            // Stable channel: check releases/latest
            let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
            match client.get(&url).header("User-Agent", "MultiClaw-Updater").send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<Value>().await {
                        let latest = body["tag_name"].as_str().unwrap_or("unknown").trim_start_matches('v');
                        let update_available = latest != CURRENT_VERSION && latest != "unknown";
                        return (StatusCode::OK, Json(json!({
                            "current_version": CURRENT_VERSION,
                            "latest_version": latest,
                            "update_available": update_available,
                            "channel": "stable",
                            "semver": CURRENT_VERSION,
                            "release_url": body["html_url"].as_str().unwrap_or("")
                        })));
                    }
                },
                _ => {}
            }
            (StatusCode::OK, Json(json!({
                "current_version": CURRENT_VERSION,
                "latest_version": CURRENT_VERSION,
                "update_available": false,
                "channel": "stable",
                "semver": CURRENT_VERSION,
                "release_url": ""
            })))
        }
    }
}

async fn system_update(State(state): State<AppState>) -> impl IntoResponse {
    let state_clone = state.clone();
    tokio::spawn(async move {
        tracing::info!("Starting system update...");
        let _ = state_clone.tx.send(json!({"type":"system_update","status":"started"}).to_string());

        // Determine which branch to pull based on update channel
        let channel: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'update_channel'")
            .fetch_optional(&state_clone.db).await.ok().flatten().unwrap_or_else(|| "stable".to_string());
        let branch = match channel.as_str() {
            "beta" => "beta",
            "dev" => "main",
            _ => "main",
        };

        // Pull latest code from the correct branch
        let pull = tokio::process::Command::new("git")
            .args(["-C", "/opt/multiclaw", "pull", "origin", branch])
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
                tracing::info!("Git pull successful (branch: {}), rebuilding containers...", branch);
            }
            Err(e) => {
                tracing::error!("Git pull error: {}", e);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                return;
            }
        }

        // Record the new deployed commit SHA.
        // Try git rev-parse first; if that fails (e.g. no .git in container),
        // fetch the latest commit SHA from GitHub API — after a successful pull,
        // HEAD matches the remote's latest commit.
        let new_sha = tokio::process::Command::new("git")
            .args(["-C", "/opt/multiclaw", "rev-parse", "HEAD"])
            .output()
            .await
            .ok()
            .and_then(|o| if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else { None });

        let new_sha = match new_sha {
            Some(sha) => sha,
            None => {
                // Fallback: fetch latest commit SHA from GitHub (matches what we just pulled)
                tracing::info!("git rev-parse failed, fetching deployed commit from GitHub API");
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new());
                let url = format!("https://api.github.com/repos/8PotatoChip8/MultiClaw/commits/{}", branch);
                let mut fetched_sha = "unknown".to_string();
                if let Ok(resp) = client.get(&url)
                    .header("User-Agent", "MultiClaw-Updater")
                    .send().await
                {
                    if let Ok(body) = resp.json::<Value>().await {
                        if let Some(sha) = body["sha"].as_str() {
                            fetched_sha = sha.to_string();
                        }
                    }
                }
                fetched_sha
            }
        };

        sqlx::query("INSERT INTO system_meta (key, value) VALUES ('deployed_commit', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
            .bind(&new_sha)
            .execute(&state_clone.db)
            .await
            .ok();
        tracing::info!("Updated deployed_commit to {}", &new_sha[..7.min(new_sha.len())]);

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

// ═══════════════════════════════════════════════════════════════
// Docker Container Status & Logs
// ═══════════════════════════════════════════════════════════════

async fn list_containers() -> impl IntoResponse {
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-a", "--format", "{{json .}}"])
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let containers: Vec<Value> = stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            (StatusCode::OK, Json(json!(containers)))
        }
        Err(e) => {
            tracing::error!("Failed to run docker ps: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Docker CLI error: {}", e)})))
        }
    }
}

#[derive(Deserialize)]
struct LogQuery {
    tail: Option<u32>,
}

async fn get_container_logs(Path(id): Path<String>, Query(q): Query<LogQuery>) -> impl IntoResponse {
    let tail = q.tail.unwrap_or(200).to_string();
    let output = tokio::process::Command::new("docker")
        .args(["logs", "--tail", &tail, "--timestamps", &id])
        .output()
        .await;

    match output {
        Ok(out) => {
            let mut logs = String::from_utf8_lossy(&out.stdout).to_string();
            // Docker logs stderr for some containers
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.is_empty() {
                logs.push_str(&stderr);
            }
            (StatusCode::OK, Json(json!({"container_id": id, "logs": logs})))
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to get logs: {}", e)})))
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent Memories
// ═══════════════════════════════════════════════════════════════

#[derive(sqlx::FromRow, serde::Serialize)]
struct AgentMemory {
    id: Uuid,
    agent_id: Uuid,
    category: String,
    key: String,
    content: String,
    importance: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

async fn get_agent_memories(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, AgentMemory>(
        "SELECT id, agent_id, category, key, content, importance, created_at, updated_at \
         FROM agent_memories WHERE agent_id = $1 ORDER BY importance DESC, updated_at DESC"
    ).bind(uid).fetch_all(&state.db).await {
        Ok(m) => (StatusCode::OK, Json(json!(m))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

#[derive(Deserialize)]
struct CreateMemoryRequest {
    category: String,
    key: String,
    content: String,
    importance: Option<i32>,
}

async fn create_agent_memory(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateMemoryRequest>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let mem_id = Uuid::new_v4();
    let importance = p.importance.unwrap_or(5);

    // Upsert: if same agent+category+key exists, update it
    match sqlx::query(
        "INSERT INTO agent_memories (id, agent_id, category, key, content, importance) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (agent_id, category, key) DO UPDATE SET content = $5, importance = $6, updated_at = NOW()"
    )
    .bind(mem_id).bind(agent_id).bind(&p.category).bind(&p.key).bind(&p.content).bind(importance)
    .execute(&state.db).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": mem_id, "status": "saved"}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

async fn delete_agent_memory(State(state): State<AppState>, Path((id, mid)): Path<(String, String)>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))) };
    let mem_id = match Uuid::parse_str(&mid) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid memory ID"}))) };
    let _ = sqlx::query("DELETE FROM agent_memories WHERE id = $1 AND agent_id = $2")
        .bind(mem_id).bind(agent_id).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"deleted"})))
}

/// Read OpenClaw's internal files (sessions, agent config) from the host filesystem.
async fn get_openclaw_files(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    let data_root = std::env::var("MULTICLAW_OPENCLAW_DATA").unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into());
    let agent_dir = std::path::Path::new(&data_root).join(agent_id.to_string());
    let config_dir = agent_dir.join("config");

    let mut files: Vec<serde_json::Value> = Vec::new();

    // Read session files
    let sessions_dir = config_dir.join("agents").join("main").join("sessions");
    if let Ok(mut entries) = tokio::fs::read_dir(&sessions_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let meta = entry.metadata().await.ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                // Read last 50 lines max for session files
                let content = if size < 100_000 {
                    tokio::fs::read_to_string(&path).await.ok()
                } else {
                    Some(format!("[File too large: {} bytes — showing is disabled]", size))
                };
                files.push(json!({
                    "name": name,
                    "path": format!("sessions/{}", name),
                    "type": "session",
                    "size": size,
                    "content": content,
                }));
            }
        }
    }

    // Read agent state files
    let agents_dir = config_dir.join("agents").join("main");
    for filename in &["state.json", "memory.json", "context.json"] {
        let fpath = agents_dir.join(filename);
        if fpath.exists() {
            let content = tokio::fs::read_to_string(&fpath).await.ok();
            let size = tokio::fs::metadata(&fpath).await.ok().map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": filename,
                "path": format!("agents/main/{}", filename),
                "type": "state",
                "size": size,
                "content": content,
            }));
        }
    }

    // Read the main config
    let config_path = config_dir.join("openclaw.json");
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await.ok();
        let size = tokio::fs::metadata(&config_path).await.ok().map(|m| m.len()).unwrap_or(0);
        files.push(json!({
            "name": "openclaw.json",
            "path": "openclaw.json",
            "type": "config",
            "size": size,
            "content": content,
        }));
    }

    // Read workspace brain files (SOUL.md, AGENTS.md, TOOLS.md, etc.)
    let workspace_dir = agent_dir.join("workspace");
    for filename in &["SOUL.md", "AGENTS.md", "TOOLS.md", "BOOTSTRAP.md", "IDENTITY.md", "USER.md"] {
        let fpath = workspace_dir.join(filename);
        if fpath.exists() {
            let content = tokio::fs::read_to_string(&fpath).await.ok();
            let size = tokio::fs::metadata(&fpath).await.ok().map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": filename,
                "path": format!("workspace/{}", filename),
                "type": "brain",
                "size": size,
                "content": content,
            }));
        }
    }
    // Read SKILL.md if present
    let skill_path = workspace_dir.join("skills").join("multiclaw").join("SKILL.md");
    if skill_path.exists() {
        let content = tokio::fs::read_to_string(&skill_path).await.ok();
        let size = tokio::fs::metadata(&skill_path).await.ok().map(|m| m.len()).unwrap_or(0);
        files.push(json!({
            "name": "SKILL.md",
            "path": "workspace/skills/multiclaw/SKILL.md",
            "type": "brain",
            "size": size,
            "content": content,
        }));
    }

    (StatusCode::OK, Json(json!(files)))
}

/// Add a participant to a thread.
async fn add_thread_participant(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<Value>) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid thread ID"}))) };
    let member_id = match p.get("member_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()) {
        Some(u) => u, None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"member_id required"})))
    };
    let member_type = p.get("member_type").and_then(|v| v.as_str()).unwrap_or("AGENT");

    match sqlx::query(
        "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
    ).bind(thread_id).bind(member_type).bind(member_id).execute(&state.db).await {
        Ok(_) => {
            let _ = state.tx.send(json!({"type":"participant_added","thread_id": thread_id, "member_id": member_id}).to_string());
            (StatusCode::CREATED, Json(json!({"status":"added", "member_id": member_id})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

/// Remove a participant from a thread.
async fn remove_thread_participant(State(state): State<AppState>, Path((id, member_id)): Path<(String, String)>) -> impl IntoResponse {
    let thread_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid thread ID"}))) };
    let mid = match Uuid::parse_str(&member_id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid member ID"}))) };

    let _ = sqlx::query("DELETE FROM thread_members WHERE thread_id = $1 AND member_id = $2")
        .bind(thread_id).bind(mid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"participant_removed","thread_id": thread_id, "member_id": mid}).to_string());
    (StatusCode::OK, Json(json!({"status":"removed"})))
}

/// Get all system settings from system_meta.
async fn get_system_settings(State(state): State<AppState>) -> impl IntoResponse {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM system_meta")
        .fetch_all(&state.db).await.unwrap_or_default();
    let mut settings = serde_json::Map::new();
    for (k, v) in rows {
        settings.insert(k, json!(v));
    }
    (StatusCode::OK, Json(json!(settings)))
}

/// Update system settings (upsert key-value pairs in system_meta).
async fn update_system_settings(State(state): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    if let Some(obj) = body.as_object() {
        for (key, val) in obj {
            let v = val.as_str().unwrap_or(&val.to_string()).to_string();
            let _ = sqlx::query(
                "INSERT INTO system_meta (key, value, updated_at) VALUES ($1, $2, NOW()) ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()"
            ).bind(key).bind(&v).execute(&state.db).await;
        }
    }
    (StatusCode::OK, Json(json!({"status":"updated"})))
}
