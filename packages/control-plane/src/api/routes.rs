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

/// Maximum reply depth for thread messages (user-to-agent or agent-to-agent in threads).
/// Prevents infinite reply chains in regular thread conversations.
const MAX_THREAD_REPLY_DEPTH: i32 = 5;

/// Safety ceiling for agent-to-agent DM conversations.
/// Conversations should end naturally via [END_CONVERSATION] signal, but this
/// hard limit prevents truly runaway loops if the signal is never produced.
const DM_SAFETY_LIMIT: i32 = 50;

/// System instructions injected into each DM turn so agents end conversations naturally.
const DM_INSTRUCTIONS: &str = "You are in a direct message conversation with a colleague. \
    Communicate naturally — ask questions, share information, and respond as needed. \
    Send ONLY your actual message to your colleague. Do NOT include your internal thoughts, \
    reasoning, planning, or thinking process — the other person sees everything you write. \
    When the conversation has reached a natural conclusion and you have nothing more to add, \
    end your final message with the exact tag [END_CONVERSATION] on its own line. \
    Do NOT use this tag if the other person asked you a question or if there are unresolved topics.";
use crate::provisioning::cloudinit::{CloudInitArgs, render_cloud_init};

/// Strip known system tags and model artifacts from agent responses.
/// Returns (cleaned_text, had_end_conversation).
pub(crate) fn strip_agent_tags(response: &str) -> (String, bool) {
    let end_conv = response.contains("[END_CONVERSATION]");
    let mut text = response.to_string();
    // Strip known system tags
    text = text.replace("[END_CONVERSATION]", "");
    text = text.replace("[HEARTBEAT_OK]", "");
    // Strip known model artifacts
    text = text.replace("[[reply_to_current]]", "");
    // Strip any remaining [[word_word]] artifacts (model-generated tags)
    while let Some(start) = text.find("[[") {
        if let Some(end) = text[start..].find("]]") {
            let tag = &text[start..start + end + 2];
            // Only strip if it looks like a simple tag (letters, underscores, hyphens)
            let inner = &tag[2..tag.len() - 2];
            if inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                text = text.replacen(tag, "", 1);
                continue;
            }
        }
        break;
    }
    (text.trim().to_string(), end_conv)
}

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
        .route("/v1/companies/:id/ledger", get(get_ledger).post(create_ledger_entry))
        .route("/v1/companies/:id/balance", get(get_balance))
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
        .route("/v1/agents/:id/vm/copy-to-sandbox", post(vm_copy_to_sandbox))
        .route("/v1/agents/:id/panic", post(agent_panic))
        .route("/v1/agents/:id/thread", get(get_agent_thread))
        .route("/v1/agents/:id/dm", post(agent_dm))
        .route("/v1/agents/:id/dm-user", post(agent_dm_user))
        .route("/v1/agents/:id/send-file", post(agent_send_file))
        .route("/v1/agents/:id/file-transfers", get(agent_file_transfers))
        .route("/v1/agents/:id/threads", get(get_agent_threads))
        .route("/v1/agents/:id/memories", get(get_agent_memories).post(create_agent_memory))
        .route("/v1/agents/:id/memories/:mid", delete(delete_agent_memory))
        .route("/v1/agents/:id/openclaw-files", get(get_openclaw_files))
        .route("/v1/agents/:id/secrets", get(list_agent_secrets))
        .route("/v1/agents/:id/secrets/:name", get(get_agent_secret))
        // Secrets
        .route("/v1/secrets", get(list_secrets).post(create_secret))
        .route("/v1/secrets/:id", delete(delete_secret))
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
        .route("/v1/requests/:id/agent-approve", post(agent_approve_request))
        .route("/v1/requests/:id/agent-reject", post(agent_reject_request))
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
        // World
        .route("/v1/world/snapshot", get(world_snapshot))
        // Events WS
        .route("/v1/events", get(events_handler))
        .layer(cors)
        .with_state(state)
}

// ═══════════════════════════════════════════════════════════════
// Health
// ═══════════════════════════════════════════════════════════════

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();
    if db_ok {
        (StatusCode::OK, Json(json!({"status": "ok", "db": "ok"})))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "degraded", "db": "unreachable"})))
    }
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

    // Store default model in system_meta for use by hire endpoints
    let _ = sqlx::query("INSERT INTO system_meta (key, value) VALUES ('default_model', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
        .bind(&model).execute(&state.db).await;

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
    let system_default: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'default_model'")
        .fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "glm-5:cloud".into());
    let model = payload.preferred_model.unwrap_or(system_default);
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

    // 1. Update DB status to QUARANTINED
    let _ = sqlx::query("UPDATE agents SET status = 'QUARANTINED' WHERE id = $1").bind(uid).execute(&state.db).await;

    // 2. Stop the OpenClaw Docker container so the agent can't execute anything
    match state.openclaw.stop_instance(uid).await {
        Ok(()) => tracing::info!("Stopped OpenClaw instance for quarantined agent {}", uid),
        Err(e) => tracing::warn!("Failed to stop OpenClaw instance for agent {}: {} (may not have one)", uid, e),
    }

    // 3. Broadcast quarantine event to UI
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
// Cross-Computer File Copy (Desktop → Sandbox)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct VmCopyToSandboxRequest {
    src_path: String,
    dest_path: Option<String>,
}

async fn vm_copy_to_sandbox(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<VmCopyToSandboxRequest>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    let desktop_ref = resolve_vm_ref(&state.db, uid, "desktop").await;
    let sandbox_ref = resolve_vm_ref(&state.db, uid, "sandbox").await;

    let (desktop_name, sandbox_name) = match (desktop_ref, sandbox_ref) {
        (Some(d), Some(s)) => (d, s),
        (None, _) => return (StatusCode::NOT_FOUND, Json(json!({"error":"No personal work computer provisioned. Provision one first."}))),
        (_, None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"No testing environment provisioned. Provision one first."}))),
    };

    let provider = match &state.vm_provider {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"VM provider not available"}))),
    };

    if body.src_path.contains("..") || body.src_path.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid src_path"})));
    }

    let dest = body.dest_path.as_deref().unwrap_or(&body.src_path);
    if dest.contains("..") || dest.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid dest_path"})));
    }

    let content = match provider.file_pull(&desktop_name, &body.src_path).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to read file from work computer: {}", e)}))),
    };

    if content.len() as u64 > MAX_FILE_TRANSFER_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({"error": format!("File too large: {} bytes (max {} bytes)", content.len(), MAX_FILE_TRANSFER_BYTES)})));
    }

    match provider.file_push(&sandbox_name, &content, dest).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status":"ok","src_path": body.src_path,"dest_path": dest,"size_bytes": content.len()}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to write file to testing environment: {}", e)}))),
    }
}

// ═══════════════════════════════════════════════════════════════
// Threads & Messages
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct ThreadsQuery {
    agent_only: Option<bool>,
}

async fn get_threads(State(state): State<AppState>, Query(q): Query<ThreadsQuery>) -> impl IntoResponse {
    if q.agent_only.unwrap_or(false) {
        // Return only threads with agent members and NO user members (agent-to-agent only)
        match sqlx::query_as::<_, Thread>(
            "SELECT DISTINCT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
             FROM threads t \
             JOIN thread_members tm ON t.id = tm.thread_id \
             WHERE tm.member_type = 'AGENT' \
               AND NOT EXISTS ( \
                   SELECT 1 FROM thread_members tm2 \
                   WHERE tm2.thread_id = t.id AND tm2.member_type = 'USER' \
               ) \
             ORDER BY t.created_at DESC"
        ).fetch_all(&state.db).await {
            Ok(t) => (StatusCode::OK, Json(json!(t))),
            Err(_) => (StatusCode::OK, Json(json!([])))
        }
    } else {
        // Return only threads where the user is a member (excludes agent-only threads)
        match sqlx::query_as::<_, Thread>(
            "SELECT DISTINCT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
             FROM threads t \
             WHERE EXISTS ( \
                 SELECT 1 FROM thread_members tm \
                 WHERE tm.thread_id = t.id AND tm.member_type = 'USER' \
             ) \
             ORDER BY t.created_at DESC"
        ).fetch_all(&state.db).await {
            Ok(t) => (StatusCode::OK, Json(json!(t))),
            Err(_) => (StatusCode::OK, Json(json!([])))
        }
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
    // Auto-add members if provided
    if let Some(member_ids) = &p.member_ids {
        for mid in member_ids {
            let _ = sqlx::query(
                "INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2) \
                 ON CONFLICT DO NOTHING"
            ).bind(id).bind(mid).execute(&state.db).await;
        }
    }
    (StatusCode::CREATED, Json(json!({"id": id, "type": p.r#type, "title": p.title})))
}

async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    match sqlx::query_as::<_, Message>("SELECT id, thread_id, sender_type, sender_id, content, reply_depth, created_at FROM messages WHERE thread_id = $1 ORDER BY created_at ASC")
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
    let reply_depth = p.reply_depth.unwrap_or(0);

    // Scrub secrets from agent-sent messages before storing
    let content = if sender_type == "AGENT" {
        if let (Some(ref crypto), Some(text)) = (&state.crypto, p.content.get("text").and_then(|v| v.as_str())) {
            let scrubbed = scrub_secrets(&state.db, crypto, sender_id, text).await;
            json!({"text": scrubbed})
        } else { p.content.clone() }
    } else { p.content.clone() };

    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,$3,$4,$5,$6) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(&sender_type).bind(sender_id).bind(&content).bind(reply_depth)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());

            // Trigger agent responses for USER or AGENT senders (with depth-based loop prevention)
            if (sender_type == "USER" || sender_type == "AGENT") && reply_depth < MAX_THREAD_REPLY_DEPTH {
                let user_text = p.content.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if !user_text.is_empty() {
                    let state_clone = state.clone();
                    let tid = thread_id;
                    let is_agent_sender = sender_type == "AGENT";
                    let next_depth = reply_depth + 1;
                    tokio::spawn(async move {
                        tracing::info!("Processing message (depth {}): '{}'", reply_depth, &user_text[..user_text.len().min(100)]);

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
                        let mut responding_agents: Vec<Uuid> = if thread_type == "GROUP" {
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

                        // If sender is an agent, exclude them from responders
                        if is_agent_sender {
                            responding_agents.retain(|id| *id != sender_id);
                        }

                        // Send to each responding agent (sequentially to avoid token waste)
                        for responding_agent_id in &responding_agents {
                            state_clone.mark_agent_working(*responding_agent_id, "Responding in thread").await;
                            let result: Result<String, anyhow::Error> = match state_clone.openclaw.send_message(
                                *responding_agent_id, &user_text,
                                Some("Respond directly to the message. Do not include your internal thoughts, reasoning, or planning process.")
                            ).await {
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
                            state_clone.mark_agent_done(*responding_agent_id).await;

                            match result {
                                Ok(response) => {
                                    // Strip system tags and model artifacts, then scrub secrets
                                    let (cleaned, _) = strip_agent_tags(&response);
                                    let scrubbed = if let Some(ref crypto) = state_clone.crypto {
                                        scrub_secrets(&state_clone.db, crypto, *responding_agent_id, &cleaned).await
                                    } else { cleaned };
                                    let resp_id = Uuid::new_v4();
                                    let content = json!({"text": scrubbed});
                                    if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                                        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5) \
                                         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                                    )
                                    .bind(resp_id)
                                    .bind(tid)
                                    .bind(responding_agent_id)
                                    .bind(&content)
                                    .bind(next_depth)
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
                                        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5)"
                                    )
                                    .bind(resp_id)
                                    .bind(tid)
                                    .bind(responding_agent_id)
                                    .bind(&content)
                                    .bind(next_depth)
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
struct RequestQuery { status: Option<String>, approver_type: Option<String> }

async fn list_requests(State(state): State<AppState>, Query(q): Query<RequestQuery>) -> impl IntoResponse {
    let status_filter = q.status.unwrap_or_else(|| "%".into());
    let approver_filter = q.approver_type.unwrap_or_else(|| "%".into());
    match sqlx::query_as::<_, Request>(
        "SELECT id, type, created_by_agent_id, created_by_user_id, company_id, payload, status, current_approver_type, current_approver_id, created_at, updated_at \
         FROM requests WHERE status LIKE $1 AND current_approver_type LIKE $2 ORDER BY created_at DESC"
    ).bind(&status_filter).bind(&approver_filter).fetch_all(&state.db).await {
        Ok(r) => (StatusCode::OK, Json(json!(r))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
}

/// Find an agent's direct superior in the chain of command.
/// Worker → Manager, Manager → CEO, CEO → MAIN, MAIN → None (user).
async fn find_superior(db: &sqlx::PgPool, agent_id: Uuid) -> Option<Uuid> {
    // First check parent_agent_id
    let parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(db).await.ok().flatten();
    if parent.is_some() {
        return parent;
    }
    // No parent — check if this is a CEO (route to MAIN) or MAIN (route to user/None)
    let role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(agent_id).fetch_optional(db).await.ok().flatten();
    match role.as_deref() {
        Some("CEO") => {
            // Route to MAIN agent
            sqlx::query_scalar("SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1")
                .fetch_optional(db).await.ok().flatten()
        }
        _ => None, // MAIN agent or unknown — route to user
    }
}

async fn create_request(State(state): State<AppState>, Json(p): Json<CreateRequestPayload>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    let requester_id = p.requester_id;

    // Scrub secrets from request payload
    let payload = if let Some(ref crypto) = state.crypto {
        let payload_str = p.payload.to_string();
        let scrubbed = scrub_secrets(&state.db, crypto, requester_id, &payload_str).await;
        serde_json::from_str(&scrubbed).unwrap_or(p.payload.clone())
    } else { p.payload.clone() };

    // Route to requester's superior in the chain of command
    let (approver_type, approver_id) = match find_superior(&state.db, requester_id).await {
        Some(superior_id) => ("AGENT".to_string(), Some(superior_id)),
        None => ("USER".to_string(), None), // MAIN agent or fallback → user
    };

    let _ = sqlx::query(
        "INSERT INTO requests (id, type, created_by_agent_id, company_id, payload, status, current_approver_type, current_approver_id) \
         VALUES ($1,$2,$3,$4,$5,'PENDING',$6,$7)"
    ).bind(id).bind(&p.r#type).bind(requester_id).bind(p.company_id).bind(&payload)
     .bind(&approver_type).bind(approver_id)
     .execute(&state.db).await;

    if approver_type == "USER" {
        // Only notify the user UI for user-targeted requests
        let _ = state.tx.send(json!({"type":"new_request","request_id": id}).to_string());
    } else if let Some(superior_id) = approver_id {
        // DM the approver agent about the pending request
        let requester_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(requester_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "An agent".into());
        let description = payload.get("description").and_then(|v| v.as_str()).unwrap_or("(no description)");
        let dm_text = format!(
            "APPROVAL REQUEST from {}: \"{}\"\n\nRequest ID: {}\nType: {}\n\n\
             To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
             To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
             -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
            requester_name, description, id, p.r#type, id, id
        );
        let state_clone = state.clone();
        tokio::spawn(async move {
            state_clone.mark_agent_working(superior_id, "Processing approval request").await;
            let _ = state_clone.openclaw.send_message(superior_id, &dm_text, None).await;
            state_clone.mark_agent_done(superior_id).await;
        });
    }

    (StatusCode::CREATED, Json(json!({"id": id, "status":"PENDING", "approver_type": approver_type})))
}

async fn approve_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Only allow user approval on requests targeting the user
    let approver_type: Option<String> = sqlx::query_scalar("SELECT current_approver_type FROM requests WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await.ok().flatten();
    if approver_type.as_deref() != Some("USER") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"This request is not awaiting user approval"})));
    }
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'APPROVE',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"request_approved","request_id": uid}).to_string());
    // Notify the requester agent
    notify_requester(&state, uid, "APPROVED", note.as_deref()).await;
    (StatusCode::OK, Json(json!({"status":"approved"})))
}

async fn reject_request(State(state): State<AppState>, Path(id): Path<String>, body: Option<Json<ApprovalAction>>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    // Only allow user rejection on requests targeting the user
    let approver_type: Option<String> = sqlx::query_scalar("SELECT current_approver_type FROM requests WHERE id = $1")
        .bind(uid).fetch_optional(&state.db).await.ok().flatten();
    if approver_type.as_deref() != Some("USER") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"This request is not awaiting user approval"})));
    }
    let note = body.and_then(|b| b.note.clone());
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'USER',$3,'REJECT',$4)")
        .bind(approval_id).bind(uid).bind(Uuid::new_v4()).bind(&note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'REJECTED', updated_at = NOW() WHERE id = $1").bind(uid).execute(&state.db).await;
    let _ = state.tx.send(json!({"type":"request_rejected","request_id": uid}).to_string());
    // Notify the requester agent
    notify_requester(&state, uid, "REJECTED", note.as_deref()).await;
    (StatusCode::OK, Json(json!({"status":"rejected"})))
}

/// Notify the original requester agent about request outcome.
async fn notify_requester(state: &AppState, request_id: Uuid, decision: &str, note: Option<&str>) {
    let requester_id: Option<Uuid> = sqlx::query_scalar("SELECT created_by_agent_id FROM requests WHERE id = $1")
        .bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    if let Some(agent_id) = requester_id {
        let req_type: String = sqlx::query_scalar("SELECT type FROM requests WHERE id = $1")
            .bind(request_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
        let note_text = note.map(|n| format!(" Note: {}", n)).unwrap_or_default();
        let msg = format!("Your request \"{}\" (ID: {}) has been {}.{}", req_type.replace('_', " "), request_id, decision, note_text);
        let state_clone = state.clone();
        tokio::spawn(async move {
            state_clone.mark_agent_working(agent_id, "Processing approval decision").await;
            let _ = state_clone.openclaw.send_message(agent_id, &msg, None).await;
            state_clone.mark_agent_done(agent_id).await;
        });
    }
}

/// Agent approves a subordinate's request. If the approving agent is MAIN, the request is
/// fully approved. Otherwise, it escalates to the next superior in the chain.
async fn agent_approve_request(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentApprovalAction>) -> impl IntoResponse {
    let request_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Verify this agent is the current approver
    let current: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT current_approver_type, current_approver_id FROM requests WHERE id = $1 AND status = 'PENDING'"
    ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    match &current {
        Some((t, Some(aid))) if t == "AGENT" && *aid == p.agent_id => {},
        _ => return (StatusCode::FORBIDDEN, Json(json!({"error":"You are not the current approver for this request"}))),
    }

    // Record this approval step
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'AGENT',$3,'APPROVE',$4)")
        .bind(approval_id).bind(request_id).bind(p.agent_id).bind(&p.note).execute(&state.db).await;

    // Check approver's role to decide: approve or escalate
    let approver_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
        .bind(p.agent_id).fetch_optional(&state.db).await.ok().flatten();

    if approver_role.as_deref() == Some("MAIN") {
        // MAIN agent (KonnerBot) has final agent authority — approve the request
        let _ = sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1")
            .bind(request_id).execute(&state.db).await;
        let _ = state.tx.send(json!({"type":"request_approved","request_id": request_id}).to_string());
        notify_requester(&state, request_id, "APPROVED", p.note.as_deref()).await;
        (StatusCode::OK, Json(json!({"status":"approved"})))
    } else {
        // Escalate to this agent's superior
        match find_superior(&state.db, p.agent_id).await {
            Some(next_superior_id) => {
                let _ = sqlx::query("UPDATE requests SET current_approver_id = $1, updated_at = NOW() WHERE id = $2")
                    .bind(next_superior_id).bind(request_id).execute(&state.db).await;

                // DM the next approver
                let approver_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                    .bind(p.agent_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
                let req_type: String = sqlx::query_scalar("SELECT type FROM requests WHERE id = $1")
                    .bind(request_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_default();
                let payload: Option<Value> = sqlx::query_scalar("SELECT payload FROM requests WHERE id = $1")
                    .bind(request_id).fetch_optional(&state.db).await.ok().flatten();
                let description = payload.as_ref().and_then(|p| p.get("description")).and_then(|v| v.as_str()).unwrap_or("(no description)");

                let dm_text = format!(
                    "APPROVAL REQUEST (escalated, approved by {}): \"{}\"\n\nRequest ID: {}\nType: {}\n\n\
                     To approve: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-approve \
                     -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'\n\n\
                     To reject: curl -s -X POST $MULTICLAW_API_URL/v1/requests/{}/agent-reject \
                     -H 'Content-Type: application/json' -d '{{\"agent_id\": \"$AGENT_ID\"}}'",
                    approver_name, description, request_id, req_type, request_id, request_id
                );
                let state_clone = state.clone();
                tokio::spawn(async move {
                    state_clone.mark_agent_working(next_superior_id, "Processing escalated approval").await;
                    let _ = state_clone.openclaw.send_message(next_superior_id, &dm_text, None).await;
                    state_clone.mark_agent_done(next_superior_id).await;
                });
                (StatusCode::OK, Json(json!({"status":"escalated","next_approver_id": next_superior_id})))
            }
            None => {
                // No superior found (shouldn't happen for non-MAIN agents, but handle gracefully)
                // Escalate to user
                let _ = sqlx::query("UPDATE requests SET current_approver_type = 'USER', current_approver_id = NULL, updated_at = NOW() WHERE id = $1")
                    .bind(request_id).execute(&state.db).await;
                let _ = state.tx.send(json!({"type":"new_request","request_id": request_id}).to_string());
                (StatusCode::OK, Json(json!({"status":"escalated_to_user"})))
            }
        }
    }
}

/// Agent rejects a subordinate's request. The request is marked as rejected immediately.
async fn agent_reject_request(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentApprovalAction>) -> impl IntoResponse {
    let request_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    // Verify this agent is the current approver
    let current: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT current_approver_type, current_approver_id FROM requests WHERE id = $1 AND status = 'PENDING'"
    ).bind(request_id).fetch_optional(&state.db).await.ok().flatten();
    match &current {
        Some((t, Some(aid))) if t == "AGENT" && *aid == p.agent_id => {},
        _ => return (StatusCode::FORBIDDEN, Json(json!({"error":"You are not the current approver for this request"}))),
    }

    // Record rejection and update status
    let approval_id = Uuid::new_v4();
    let _ = sqlx::query("INSERT INTO approvals (id, request_id, approver_type, approver_id, decision, note) VALUES ($1,$2,'AGENT',$3,'REJECT',$4)")
        .bind(approval_id).bind(request_id).bind(p.agent_id).bind(&p.note).execute(&state.db).await;
    let _ = sqlx::query("UPDATE requests SET status = 'REJECTED', updated_at = NOW() WHERE id = $1")
        .bind(request_id).execute(&state.db).await;

    // Notify requester
    notify_requester(&state, request_id, "REJECTED", p.note.as_deref()).await;
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

    // Auto-record paired ledger entries for the engagement
    let engagement: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT client_company_id, provider_company_id, service_id FROM service_engagements WHERE id = $1"
    ).bind(uid).fetch_optional(&state.db).await.ok().flatten();

    if let Some((client_id, provider_id, service_id)) = engagement {
        let svc: Option<(String, Value)> = sqlx::query_as(
            "SELECT name, rate FROM service_catalog WHERE id = $1"
        ).bind(service_id).fetch_optional(&state.db).await.ok().flatten();

        if let Some((service_name, rate)) = svc {
            let amount = rate["amount"].as_f64().unwrap_or(0.0);
            let currency = rate["currency"].as_str().unwrap_or("USD").to_string();

            if amount > 0.0 {
                let amount_str = format!("{}", amount);
                let expense_memo = format!("Service: {} (engagement completed)", service_name);
                let revenue_memo = format!("Service: {} (engagement completed)", service_name);

                // EXPENSE for client
                let _ = sqlx::query(
                    "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                     VALUES ($1, $2, $3, $4, 'EXPENSE', $5::NUMERIC, $6, $7, true)"
                ).bind(Uuid::new_v4()).bind(client_id).bind(Some(provider_id)).bind(Some(uid))
                 .bind(&amount_str).bind(&currency).bind(&expense_memo)
                 .execute(&state.db).await;

                // REVENUE for provider
                let _ = sqlx::query(
                    "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                     VALUES ($1, $2, $3, $4, 'REVENUE', $5::NUMERIC, $6, $7, true)"
                ).bind(Uuid::new_v4()).bind(provider_id).bind(Some(client_id)).bind(Some(uid))
                 .bind(&amount_str).bind(&currency).bind(&revenue_memo)
                 .execute(&state.db).await;
            }
        }
    }

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

#[derive(Debug, Deserialize)]
struct CreateLedgerEntryRequest {
    r#type: String,
    amount: f64,
    currency: String,
    memo: Option<String>,
    counterparty_company_id: Option<String>,
    engagement_id: Option<String>,
}

async fn create_ledger_entry(State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<CreateLedgerEntryRequest>) -> impl IntoResponse {
    let company_id = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };
    let valid_types = ["EXPENSE", "REVENUE", "INTERNAL_TRANSFER", "CAPITAL_INJECTION"];
    if !valid_types.contains(&p.r#type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid type. Must be one of: {}", valid_types.join(", "))})));
    }
    if p.amount <= 0.0 {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Amount must be positive"})));
    }

    let counterparty_id = p.counterparty_company_id.as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());
    let engagement_id = p.engagement_id.as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());

    let entry_id = Uuid::new_v4();
    let amount_str = format!("{}", p.amount);

    if let Err(e) = sqlx::query(
        "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
         VALUES ($1, $2, $3, $4, $5, $6::NUMERIC, $7, $8, true)"
    ).bind(entry_id).bind(company_id).bind(counterparty_id).bind(engagement_id)
     .bind(&p.r#type).bind(&amount_str).bind(&p.currency).bind(&p.memo)
     .execute(&state.db).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})));
    }

    // For INTERNAL_TRANSFER, create the paired entry on the counterparty
    if p.r#type == "INTERNAL_TRANSFER" {
        if let Some(cp_id) = counterparty_id {
            let paired_id = Uuid::new_v4();
            let paired_memo = p.memo.as_deref().map(|m| format!("Transfer from counterparty: {}", m))
                .unwrap_or_else(|| "Transfer from counterparty".to_string());
            let _ = sqlx::query(
                "INSERT INTO ledger_entries (id, company_id, counterparty_company_id, engagement_id, type, amount, currency, memo, is_virtual) \
                 VALUES ($1, $2, $3, $4, 'REVENUE', $5::NUMERIC, $6, $7, true)"
            ).bind(paired_id).bind(cp_id).bind(Some(company_id)).bind(engagement_id)
             .bind(&amount_str).bind(&p.currency).bind(&paired_memo)
             .execute(&state.db).await;
        }
    }

    (StatusCode::CREATED, Json(json!({"id": entry_id})))
}

async fn get_balance(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let uid = match Uuid::parse_str(&id) { Ok(u) => u, Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))) };

    let rows: Vec<(String, String, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT currency, type, COALESCE(SUM(amount), 0) as total \
         FROM ledger_entries WHERE company_id = $1 GROUP BY currency, type"
    ).bind(uid).fetch_all(&state.db).await.unwrap_or_default();

    let mut balances: serde_json::Map<String, Value> = serde_json::Map::new();
    for (currency, entry_type, total) in &rows {
        let currency_obj = balances.entry(currency.clone())
            .or_insert_with(|| json!({"revenue": 0.0, "expenses": 0.0, "capital": 0.0, "net": 0.0}));
        let total_f64 = total.to_string().parse::<f64>().unwrap_or(0.0);
        match entry_type.as_str() {
            "REVENUE" => { currency_obj["revenue"] = json!(total_f64); }
            "EXPENSE" => { currency_obj["expenses"] = json!(total_f64); }
            "CAPITAL_INJECTION" => { currency_obj["capital"] = json!(total_f64); }
            "INTERNAL_TRANSFER" => { currency_obj["expenses"] = json!(currency_obj["expenses"].as_f64().unwrap_or(0.0) + total_f64); }
            _ => {}
        }
    }
    // Calculate net for each currency
    for (_, obj) in balances.iter_mut() {
        let revenue = obj["revenue"].as_f64().unwrap_or(0.0);
        let expenses = obj["expenses"].as_f64().unwrap_or(0.0);
        let capital = obj["capital"].as_f64().unwrap_or(0.0);
        obj["net"] = json!(capital + revenue - expenses);
    }

    (StatusCode::OK, Json(json!(balances)))
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
// Agent-to-Agent DM
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct AgentDmRequest {
    target: String,   // agent UUID or handle (e.g. "@ceo-acme")
    message: String,
}

async fn agent_dm(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentDmRequest>
) -> impl IntoResponse {
    let sender_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid sender ID"}))),
    };

    // Resolve target: UUID or @handle
    let target_id: Uuid = if p.target.starts_with('@') {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE handle = $1")
            .bind(&p.target).fetch_optional(&state.db).await {
            Ok(Some(id)) => id,
            _ => return (StatusCode::NOT_FOUND, Json(json!({"error": format!("Agent with handle '{}' not found", p.target)}))),
        }
    } else {
        match Uuid::parse_str(&p.target) {
            Ok(u) => u,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid target — use a UUID or @handle"}))),
        }
    };

    if sender_id == target_id {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Cannot DM yourself"})));
    }

    // Block DMs involving quarantined agents
    let sender_status: Option<String> = sqlx::query_scalar("SELECT status FROM agents WHERE id = $1")
        .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
    let target_status: Option<String> = sqlx::query_scalar("SELECT status FROM agents WHERE id = $1")
        .bind(target_id).fetch_optional(&state.db).await.ok().flatten();
    if sender_status.as_deref() == Some("QUARANTINED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Sender agent is quarantined and cannot send messages"})));
    }
    if target_status.as_deref() == Some("QUARANTINED") {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"Target agent is quarantined and cannot receive messages"})));
    }

    // Enforce communication hierarchy
    {
        let sender_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
            .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
        let target_role: Option<String> = sqlx::query_scalar("SELECT role FROM agents WHERE id = $1")
            .bind(target_id).fetch_optional(&state.db).await.ok().flatten();

        // MAIN can DM anyone
        if sender_role.as_deref() != Some("MAIN") {
            let sender_company: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1")
                .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
            let target_company: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1")
                .bind(target_id).fetch_optional(&state.db).await.ok().flatten();
            let sender_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
                .bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
            let target_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
                .bind(target_id).fetch_optional(&state.db).await.ok().flatten();

            let allowed = match (sender_role.as_deref(), target_role.as_deref()) {
                // Only CEOs can DM MAIN
                (Some("CEO"), Some("MAIN")) => true,
                (_, Some("MAIN")) => false,
                // Can DM your direct parent
                _ if sender_parent == Some(target_id) => true,
                // Can DM your direct child
                _ if target_parent == Some(sender_id) => true,
                // CEO can DM any agent in their company
                (Some("CEO"), _) if sender_company == target_company && sender_company.is_some() => true,
                // Peers under the same parent in the same company
                _ if sender_company == target_company && sender_parent == target_parent
                    && sender_company.is_some() => true,
                _ => false,
            };

            if !allowed {
                return (StatusCode::FORBIDDEN, Json(json!({
                    "error": "You can only message agents in your direct chain of command. Escalate through your superior."
                })));
            }
        }
    }

    // Anti-spam: short cooldown to prevent rapid DM re-initiation between same pair
    let pair_key = if sender_id < target_id { (sender_id, target_id) } else { (target_id, sender_id) };
    {
        let cooldowns = state.dm_cooldowns.read().await;
        if let Some(last_completed) = cooldowns.get(&pair_key) {
            let elapsed = last_completed.elapsed().as_secs();
            if elapsed < 10 {
                return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                    "error": "A DM conversation between these agents just concluded. Please wait a moment.",
                    "cooldown_remaining_secs": 10 - elapsed
                })));
            }
        }
    }

    // Rate limit: max 10 agent messages per minute per sender
    let recent_count: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND sender_type = 'AGENT' AND created_at > NOW() - INTERVAL '1 minute'"
    ).bind(sender_id).fetch_optional(&state.db).await.ok().flatten();
    if recent_count.unwrap_or(0) >= 10 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":"Rate limit exceeded — max 10 agent messages per minute"})));
    }

    // Prevent concurrent DM conversations between the same pair
    {
        let active = state.active_dm_pairs.read().await;
        if active.contains(&pair_key) {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "error": "A DM conversation between these agents is already in progress."
            })));
        }
    }
    {
        let mut active = state.active_dm_pairs.write().await;
        active.insert(pair_key);
    }

    // Find existing agent-to-agent DM (no USER members)
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM threads t \
         JOIN thread_members tm1 ON t.id = tm1.thread_id \
         JOIN thread_members tm2 ON t.id = tm2.thread_id \
         WHERE t.type = 'DM' \
           AND tm1.member_type = 'AGENT' AND tm1.member_id = $1 \
           AND tm2.member_type = 'AGENT' AND tm2.member_id = $2 \
           AND NOT EXISTS ( \
               SELECT 1 FROM thread_members tm3 \
               WHERE tm3.thread_id = t.id AND tm3.member_type = 'USER' \
           ) \
         LIMIT 1"
    ).bind(sender_id).bind(target_id).fetch_optional(&state.db).await.unwrap_or(None);

    let thread_id = if let Some((tid,)) = existing {
        tid
    } else {
        // Create new agent-to-agent DM thread
        let sender_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(sender_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let target_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(target_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let tid = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
            .bind(tid).bind(format!("{} <-> {}", sender_name, target_name))
            .execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(sender_id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(target_id).execute(&state.db).await;
        tid
    };

    // Insert message (scrub secrets before storing)
    let msg_id = Uuid::new_v4();
    let scrubbed_msg = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, sender_id, &p.message).await
    } else { p.message.clone() };
    let content = json!({"text": scrubbed_msg});
    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,0) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(sender_id).bind(&content)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());

            // Conversation loop: agents take turns responding until conversation ends naturally
            let state_clone = state.clone();
            let text = p.message.clone();
            tokio::spawn(async move {
                let mut current_depth: i32 = 0;
                let mut responder_id = target_id;
                let mut other_id = sender_id;
                let mut current_text = text;

                loop {
                    // Check if either agent was quarantined during conversation
                    let responder_status: Option<String> = sqlx::query_scalar(
                        "SELECT status FROM agents WHERE id = $1"
                    ).bind(responder_id).fetch_optional(&state_clone.db).await.ok().flatten();
                    if responder_status.as_deref() == Some("QUARANTINED") {
                        tracing::info!("DM conversation on thread {} stopped: agent {} is quarantined", thread_id, responder_id);
                        break;
                    }

                    state_clone.mark_agent_working(responder_id, "Chatting in DM").await;
                    let result = state_clone.openclaw.send_message(responder_id, &current_text, Some(DM_INSTRUCTIONS)).await;
                    state_clone.mark_agent_done(responder_id).await;
                    current_depth += 1;

                    match result {
                        Ok(response) => {
                            // Strip system tags and model artifacts from response
                            let (clean_response, conversation_complete) = strip_agent_tags(&response);

                            // Scrub secrets before storing (use clean response without tag)
                            let scrubbed = if let Some(ref crypto) = state_clone.crypto {
                                scrub_secrets(&state_clone.db, crypto, responder_id, &clean_response).await
                            } else { clean_response.clone() };
                            let resp_id = Uuid::new_v4();
                            let resp_content = json!({"text": scrubbed});
                            if let Ok(agent_msg) = sqlx::query_as::<_, Message>(
                                "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,$5) \
                                 RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
                            ).bind(resp_id).bind(thread_id).bind(responder_id).bind(&resp_content).bind(current_depth)
                            .fetch_one(&state_clone.db).await {
                                let _ = state_clone.tx.send(json!({"type":"new_message","message": agent_msg}).to_string());
                            }

                            // End if the agent signaled conversation is complete
                            if conversation_complete {
                                tracing::info!("DM conversation on thread {} ended naturally at depth {}", thread_id, current_depth);
                                break;
                            }

                            // Safety ceiling to prevent truly runaway conversations
                            if current_depth >= DM_SAFETY_LIMIT {
                                tracing::warn!("DM conversation on thread {} hit safety limit {}", thread_id, DM_SAFETY_LIMIT);
                                break;
                            }

                            // Swap roles: the other agent now responds to this one's message
                            std::mem::swap(&mut responder_id, &mut other_id);
                            current_text = clean_response;
                        }
                        Err(e) => {
                            tracing::warn!("OpenClaw unavailable for agent {}: {}", responder_id, e);
                            let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
                                .bind(responder_id).fetch_optional(&state_clone.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());

                            // Post a SYSTEM message so it shows in the thread UI
                            let resp_id = Uuid::new_v4();
                            let resp_content = json!({"text": format!("{} is currently unavailable. Your message was not delivered — please try again later.", agent_name)});
                            let _ = sqlx::query(
                                "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'SYSTEM',$3,$4,$5)"
                            ).bind(resp_id).bind(thread_id).bind(responder_id).bind(&resp_content).bind(current_depth)
                            .execute(&state_clone.db).await;
                            let _ = state_clone.tx.send(json!({"type":"new_message","thread_id": thread_id, "system": true, "text": format!("{} is currently unavailable.", agent_name)}).to_string());

                            // Notify the sender agent that delivery failed so they can retry
                            let failure_notice = format!(
                                "Your message to {} was NOT delivered — they are currently unavailable. \
                                 You should retry sending this message later when they come back online.",
                                agent_name
                            );
                            state_clone.mark_agent_working(other_id, "Processing delivery failure").await;
                            let _ = state_clone.openclaw.send_message(other_id, &failure_notice, None).await;
                            state_clone.mark_agent_done(other_id).await;
                            break;
                        }
                    }
                }

                // Mark both agents as idle when the DM conversation ends
                state_clone.mark_agent_done(sender_id).await;
                state_clone.mark_agent_done(target_id).await;

                // Record cooldown for this agent pair to prevent infinite re-initiation
                let pair_key = if sender_id < target_id { (sender_id, target_id) } else { (target_id, sender_id) };
                {
                    let mut cooldowns = state_clone.dm_cooldowns.write().await;
                    cooldowns.insert(pair_key, tokio::time::Instant::now());
                }
                // Release active-conversation lock
                {
                    let mut active = state_clone.active_dm_pairs.write().await;
                    active.remove(&pair_key);
                }
            });

            (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "message_id": msg_id})))
        }
        Err(e) => {
            // Release active-conversation lock on failure
            {
                let mut active = state.active_dm_pairs.write().await;
                active.remove(&pair_key);
            }
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent-to-User DM
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct AgentDmUserRequest {
    message: String,
}

/// Allows an agent to send a DM to the human operator.
/// Creates/finds a DM thread that includes a USER member so it shows
/// up in the operator's thread list.
async fn agent_dm_user(
    State(state): State<AppState>, Path(id): Path<String>, Json(p): Json<AgentDmUserRequest>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };

    // Rate limit: max 5 user-directed messages per minute per agent
    let recent_count: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND sender_type = 'AGENT' AND created_at > NOW() - INTERVAL '1 minute' \
         AND thread_id IN (SELECT thread_id FROM thread_members WHERE member_type = 'USER')"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();
    if recent_count.unwrap_or(0) >= 5 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":"Rate limit exceeded — max 5 user messages per minute"})));
    }

    // Find existing user-agent DM thread
    let user_id = Uuid::from_u128(0); // placeholder user ID (matches get_agent_thread)
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM threads t \
         JOIN thread_members tm1 ON t.id = tm1.thread_id \
         JOIN thread_members tm2 ON t.id = tm2.thread_id \
         WHERE t.type = 'DM' \
           AND tm1.member_type = 'AGENT' AND tm1.member_id = $1 \
           AND tm2.member_type = 'USER' AND tm2.member_id = $2 \
         LIMIT 1"
    ).bind(agent_id).bind(user_id).fetch_optional(&state.db).await.unwrap_or(None);

    let thread_id = if let Some((tid,)) = existing {
        tid
    } else {
        // Create new user-agent DM thread
        let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
            .bind(agent_id).fetch_optional(&state.db).await.ok().flatten().unwrap_or_else(|| "Agent".into());
        let tid = Uuid::new_v4();
        let _ = sqlx::query("INSERT INTO threads (id, type, title) VALUES ($1, 'DM', $2)")
            .bind(tid).bind(format!("DM with {}", agent_name))
            .execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'AGENT', $2)")
            .bind(tid).bind(agent_id).execute(&state.db).await;
        let _ = sqlx::query("INSERT INTO thread_members (thread_id, member_type, member_id) VALUES ($1, 'USER', $2)")
            .bind(tid).bind(user_id).execute(&state.db).await;
        tid
    };

    // Scrub secrets from agent message before storing
    let scrubbed_message = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, agent_id, &p.message).await
    } else { p.message.clone() };

    // Insert the agent's message
    let msg_id = Uuid::new_v4();
    let content = json!({"text": scrubbed_message});
    match sqlx::query_as::<_, Message>(
        "INSERT INTO messages (id, thread_id, sender_type, sender_id, content, reply_depth) VALUES ($1,$2,'AGENT',$3,$4,0) \
         RETURNING id, thread_id, sender_type, sender_id, content, reply_depth, created_at"
    ).bind(msg_id).bind(thread_id).bind(agent_id).bind(&content)
    .fetch_one(&state.db).await {
        Ok(msg) => {
            let _ = state.tx.send(json!({"type":"new_message","message": msg}).to_string());
            (StatusCode::CREATED, Json(json!({"thread_id": thread_id, "message_id": msg_id, "status": "delivered"})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    }
}

// ═══════════════════════════════════════════════════════════════
// Inter-Agent File Transfer
// ═══════════════════════════════════════════════════════════════

/// Maximum file size for inter-agent transfers: 10 MB
const MAX_FILE_TRANSFER_BYTES: u64 = 10 * 1024 * 1024;

fn parse_role(role_str: &str) -> crate::policy::engine::Role {
    use crate::policy::engine::Role;
    match role_str {
        "MAIN"    => Role::Main,
        "CEO"     => Role::Ceo,
        "MANAGER" => Role::Manager,
        _         => Role::Worker,
    }
}

async fn agent_send_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(p): Json<AgentFileSendRequest>,
) -> impl IntoResponse {
    let sender_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid sender ID"}))),
    };

    // Resolve target: UUID or @handle
    let receiver_id: Uuid = if p.target.starts_with('@') {
        match sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE handle = $1")
            .bind(&p.target).fetch_optional(&state.db).await
        {
            Ok(Some(id)) => id,
            _ => return (StatusCode::NOT_FOUND, Json(json!({"error": format!("Agent '{}' not found", p.target)}))),
        }
    } else {
        match Uuid::parse_str(&p.target) {
            Ok(u) => u,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid target — use UUID or @handle"}))),
        }
    };

    if sender_id == receiver_id {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Cannot send a file to yourself"})));
    }

    // Fetch both agents
    let sender: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, \
         preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, \
         sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1"
    ).bind(sender_id).fetch_optional(&state.db).await.ok().flatten();

    let receiver: Option<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, \
         preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, \
         sandbox_vm_id, handle, status, created_at FROM agents WHERE id = $1"
    ).bind(receiver_id).fetch_optional(&state.db).await.ok().flatten();

    let (sender, receiver) = match (sender, receiver) {
        (Some(s), Some(r)) => (s, r),
        (None, _) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Sender agent not found"}))),
        (_, None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Receiver agent not found"}))),
    };

    // Policy check
    let ctx = crate::policy::engine::FileTransferContext {
        sender_role: parse_role(&sender.role),
        receiver_role: parse_role(&receiver.role),
        sender_id,
        receiver_id,
        sender_parent: sender.parent_agent_id,
        receiver_parent: receiver.parent_agent_id,
        sender_company: sender.company_id,
        receiver_company: receiver.company_id,
    };

    match crate::policy::engine::can_send_file(&ctx) {
        crate::policy::engine::Decision::Denied(reason) => {
            return (StatusCode::FORBIDDEN, Json(json!({"error": reason})));
        }
        crate::policy::engine::Decision::AllowedImmediate => {}
        _ => {
            return (StatusCode::FORBIDDEN, Json(json!({"error":"File transfer not permitted"})));
        }
    }

    // Sanitize src_path
    let src_path = p.src_path.trim_start_matches('/');
    if src_path.contains("..") || src_path.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid src_path"})));
    }

    // Build host filesystem paths
    let data_root = std::env::var("MULTICLAW_OPENCLAW_DATA")
        .unwrap_or_else(|_| "/opt/multiclaw/openclaw-data".into());

    let sender_workspace = std::path::PathBuf::from(&data_root)
        .join(sender_id.to_string())
        .join("workspace");
    let src_file = sender_workspace.join(src_path);

    // Verify path stays inside workspace
    let src_canonical = match src_file.canonicalize() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::NOT_FOUND, Json(json!({
                "error": format!("File not found in sender workspace: {}", src_path)
            })));
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Path error: {}", e)})));
        }
    };
    if !src_canonical.starts_with(&sender_workspace) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"src_path escapes workspace boundary"})));
    }

    // Read and enforce size limit
    let file_bytes = match tokio::fs::read(&src_canonical).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to read file: {}", e)}))),
    };

    if file_bytes.len() as u64 > MAX_FILE_TRANSFER_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({
            "error": format!("File too large: {} bytes (max {} bytes)", file_bytes.len(), MAX_FILE_TRANSFER_BYTES)
        })));
    }

    let size_bytes = file_bytes.len() as i64;
    let encoding = p.encoding.as_deref().unwrap_or("text");

    // Determine destination path
    let filename = std::path::Path::new(src_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(src_path);
    let dest_relative = p.dest_path.as_deref().unwrap_or(filename);
    let dest_relative = dest_relative.trim_start_matches('/');
    if dest_relative.contains("..") || dest_relative.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid dest_path"})));
    }

    let receiver_workspace = std::path::PathBuf::from(&data_root)
        .join(receiver_id.to_string())
        .join("workspace");
    let dest_file = receiver_workspace.join(dest_relative);

    // Verify dest stays inside workspace (pre-creation check via join logic)
    if !dest_file.starts_with(&receiver_workspace) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"dest_path escapes workspace boundary"})));
    }

    // Create parent directories if needed, then write
    if let Some(parent) = dest_file.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to create destination directory: {}", e)
            })));
        }
    }

    if let Err(e) = tokio::fs::write(&dest_file, &file_bytes).await {
        let transfer_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO file_transfers (id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, status, error) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,'FAILED',$8)"
        ).bind(transfer_id).bind(sender_id).bind(receiver_id)
         .bind(filename).bind(size_bytes).bind(encoding)
         .bind(dest_relative).bind(e.to_string())
         .execute(&state.db).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to write file: {}", e)})));
    }

    // Record successful transfer
    let transfer_id = Uuid::new_v4();
    let _ = sqlx::query(
        "INSERT INTO file_transfers (id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, status) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'DELIVERED')"
    ).bind(transfer_id).bind(sender_id).bind(receiver_id)
     .bind(filename).bind(size_bytes).bind(encoding).bind(dest_relative)
     .execute(&state.db).await;

    // Notify receiver
    let sender_name = sender.name.clone();
    let notify_dest = dest_relative.to_string();
    let notify_filename = filename.to_string();
    let notify_size = file_bytes.len();
    let state_clone = state.clone();
    tokio::spawn(async move {
        let msg = format!(
            "FILE RECEIVED from {}: '{}' has been placed in your workspace at '/workspace/{}' ({} bytes).",
            sender_name, notify_filename, notify_dest, notify_size
        );
        state_clone.mark_agent_working(receiver_id, "Processing received file").await;
        let _ = state_clone.openclaw.send_message(receiver_id, &msg, None).await;
        state_clone.mark_agent_done(receiver_id).await;
    });

    // Broadcast WebSocket event
    let _ = state.tx.send(json!({
        "type": "file_transferred",
        "transfer_id": transfer_id,
        "sender_id": sender_id,
        "receiver_id": receiver_id,
        "filename": filename,
        "dest_path": dest_relative,
    }).to_string());

    tracing::info!("File '{}' transferred from {} to {} ({} bytes)", filename, sender.name, receiver.name, notify_size);

    (StatusCode::CREATED, Json(json!({
        "transfer_id": transfer_id,
        "status": "delivered",
        "filename": filename,
        "dest_path": dest_relative,
        "size_bytes": notify_size,
    })))
}

async fn agent_file_transfers(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    match sqlx::query_as::<_, FileTransfer>(
        "SELECT id, sender_id, receiver_id, filename, size_bytes, encoding, dest_path, \
         status, error, created_at \
         FROM file_transfers \
         WHERE sender_id = $1 OR receiver_id = $1 \
         ORDER BY created_at DESC LIMIT 100"
    ).bind(agent_id).fetch_all(&state.db).await {
        Ok(transfers) => (StatusCode::OK, Json(json!(transfers))),
        Err(_) => (StatusCode::OK, Json(json!([]))),
    }
}

// ═══════════════════════════════════════════════════════════════
// Agent's Own Threads
// ═══════════════════════════════════════════════════════════════

async fn get_agent_threads(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    match sqlx::query_as::<_, Thread>(
        "SELECT t.id, t.type, t.title, t.created_by_user_id, t.created_at \
         FROM threads t \
         JOIN thread_members tm ON t.id = tm.thread_id \
         WHERE tm.member_type = 'AGENT' AND tm.member_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT 50"
    ).bind(agent_id).fetch_all(&state.db).await {
        Ok(t) => (StatusCode::OK, Json(json!(t))),
        Err(_) => (StatusCode::OK, Json(json!([])))
    }
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

/// Simple semver greater-than comparison (a > b).
fn semver_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut parts = s.split('.').filter_map(|p| p.parse().ok());
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    };
    parse(a) > parse(b)
}

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
                        let update_available = latest != "unknown" && semver_gt(latest, CURRENT_VERSION);
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

        // Determine which branch/tag to pull based on update channel
        let channel: String = sqlx::query_scalar("SELECT value FROM system_meta WHERE key = 'update_channel'")
            .fetch_optional(&state_clone.db).await.ok().flatten().unwrap_or_else(|| "stable".to_string());

        let is_stable = !matches!(channel.as_str(), "beta" | "dev");

        if is_stable {
            // Stable channel: fetch the latest release tag from GitHub and checkout that tag
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let tag = match client.get("https://api.github.com/repos/8PotatoChip8/MultiClaw/releases/latest")
                .header("User-Agent", "MultiClaw-Updater")
                .send().await
            {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<Value>().await.ok()
                        .and_then(|body| body["tag_name"].as_str().map(String::from))
                },
                _ => None,
            };

            let tag = match tag {
                Some(t) => t,
                None => {
                    tracing::error!("Failed to fetch latest release tag from GitHub");
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error":"Could not fetch latest release tag"}).to_string());
                    return;
                }
            };

            tracing::info!("Stable update: checking out release tag {}", tag);

            // Unshallow the repo if it was cloned with --depth 1 (install-stable.sh).
            // Shallow clones may not be able to fetch tags pointing to commits outside the shallow history.
            let is_shallow = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--is-shallow-repository"])
                .output()
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false);

            if is_shallow {
                tracing::info!("Repo is shallow, unshallowing before tag fetch...");
                let _ = tokio::process::Command::new("git")
                    .args(["-C", "/opt/multiclaw", "fetch", "--unshallow", "origin"])
                    .output()
                    .await;
            }

            // Fetch all tags
            let fetch = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "fetch", "origin", "--tags", "--force"])
                .output()
                .await;

            match fetch {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git fetch tags successful");
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git fetch tags failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git fetch tags error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }

            // Checkout the release tag
            let checkout = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "checkout", &tag])
                .output()
                .await;

            match checkout {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git checkout {} successful, rebuilding containers...", tag);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git checkout {} failed: {}", tag, err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git checkout error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }
        } else {
            // Dev/beta channels: fetch branch and hard-reset to it.
            // Using fetch+reset instead of pull avoids merge conflicts if tracked
            // files were locally modified. reset --hard only affects tracked files;
            // untracked dirs like openclaw-data/ are untouched.
            let branch = if channel == "beta" { "beta" } else { "main" };

            // Unshallow the repo if it was cloned with --depth 1 (install-stable.sh).
            // Shallow clones don't have remote tracking refs, so `origin/main` doesn't exist.
            let is_shallow = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--is-shallow-repository"])
                .output()
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false);

            if is_shallow {
                tracing::info!("Repo is shallow, unshallowing before fetch...");
                let _ = tokio::process::Command::new("git")
                    .args(["-C", "/opt/multiclaw", "fetch", "--unshallow", "origin"])
                    .output()
                    .await;
            }

            let fetch = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "fetch", "origin", branch])
                .output()
                .await;

            match fetch {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git fetch successful (branch: {})", branch);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git fetch failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git fetch error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }

            // Use FETCH_HEAD as the reset target — it always exists after a fetch,
            // even on shallow clones where origin/<branch> refs may not be created.
            let reset = tokio::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "reset", "--hard", "FETCH_HEAD"])
                .output()
                .await;

            match reset {
                Ok(output) if output.status.success() => {
                    tracing::info!("Git reset to FETCH_HEAD ({}) successful, rebuilding containers...", branch);
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    tracing::error!("Git reset to FETCH_HEAD failed: {}", err);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                    return;
                }
                Err(e) => {
                    tracing::error!("Git reset error: {}", e);
                    let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                    return;
                }
            }
        }

        // Record the new deployed commit SHA after successful pull.
        // /opt/multiclaw is volume-mounted with .git available.
        let new_sha = tokio::process::Command::new("git")
            .args(["-C", "/opt/multiclaw", "rev-parse", "HEAD"])
            .output()
            .await
            .ok()
            .and_then(|o| if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else { None })
            .unwrap_or_else(|| "unknown".to_string());

        sqlx::query("INSERT INTO system_meta (key, value) VALUES ('deployed_commit', $1) ON CONFLICT (key) DO UPDATE SET value = $1, updated_at = NOW()")
            .bind(&new_sha)
            .execute(&state_clone.db)
            .await
            .ok();
        tracing::info!("Updated deployed_commit to {}", &new_sha[..7.min(new_sha.len())]);

        // Rebuild and restart containers via a DETACHED ephemeral container.
        // We cannot run `docker compose up -d --build` directly because it will
        // replace THIS container (multiclawd) mid-execution, killing the compose
        // process before it can recreate the remaining services (ui, ollama-proxy).
        // By running it from a separate container, the rebuild survives our replacement.

        // Clean up any leftover updater from a previous run
        let _ = tokio::process::Command::new("docker")
            .args(["rm", "-f", "multiclaw-updater"])
            .output()
            .await;

        // The updater container also rebuilds the CLI after compose finishes.
        // cli_build_cmd runs inside the same ephemeral container sequentially.
        let rebuild = tokio::process::Command::new("docker")
            .args([
                "run", "-d", "--rm",
                "--name", "multiclaw-updater",
                "-v", "/var/run/docker.sock:/var/run/docker.sock",
                "-v", "/opt/multiclaw:/opt/multiclaw",
                "docker:cli",
                "sh", "-c",
                "docker compose -f /opt/multiclaw/infra/docker/docker-compose.yml up -d --build \
                 && docker run --rm \
                    -v /opt/multiclaw/packages:/usr/src/app/packages \
                    rust:1-slim-bookworm \
                    bash -c 'apt-get update && apt-get install -y pkg-config libssl-dev > /dev/null 2>&1 && cd /usr/src/app/packages && cargo build --release -p multiclaw-cli' \
                 || true"
            ])
            .output()
            .await;

        match rebuild {
            Ok(output) if output.status.success() => {
                tracing::info!("Updater container launched — rebuild will continue independently");
            }
            Ok(output) => {
                let err = String::from_utf8_lossy(&output.stderr);
                tracing::error!("Failed to launch updater container: {}", err);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": err.to_string()}).to_string());
                return;
            }
            Err(e) => {
                tracing::error!("Failed to launch updater container: {}", e);
                let _ = state_clone.tx.send(json!({"type":"system_update","status":"failed","error": e.to_string()}).to_string());
                return;
            }
        }

        // Note: "complete" message may never reach the client because multiclawd
        // will be replaced by the updater. The frontend already handles this by
        // polling /v1/health and reloading when the new container is up.
        tracing::info!("System update handed off to updater container");
        let _ = state_clone.tx.send(json!({"type":"system_update","status":"complete"}).to_string());
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

    // Scrub secrets from memory content before storing
    let content = if let Some(ref crypto) = state.crypto {
        scrub_secrets(&state.db, crypto, agent_id, &p.content).await
    } else { p.content.clone() };

    // Upsert: if same agent+category+key exists, update it
    match sqlx::query(
        "INSERT INTO agent_memories (id, agent_id, category, key, content, importance) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (agent_id, category, key) DO UPDATE SET content = $5, importance = $6, updated_at = NOW()"
    )
    .bind(mem_id).bind(agent_id).bind(&p.category).bind(&p.key).bind(&content).bind(importance)
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

// ═══════════════════════════════════════════════════════════════
// Secrets Management
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct CreateSecretRequest {
    scope_type: String,  // "agent", "manager", "company", "holding"
    scope_id: Uuid,
    name: String,        // e.g., "coinex_api_key"
    value: String,       // plaintext — will be encrypted before storage
    description: Option<String>, // human-readable description of what the secret is for
}

#[derive(Debug, Deserialize)]
struct SecretsQuery {
    scope_type: Option<String>,
    scope_id: Option<Uuid>,
}

async fn create_secret(
    State(state): State<AppState>, Json(p): Json<CreateSecretRequest>
) -> impl IntoResponse {
    let crypto = match &state.crypto {
        Some(c) => c,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Secrets not available — master key not configured"}))),
    };

    // Validate scope_type
    if !["agent", "company", "holding", "manager"].contains(&p.scope_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"scope_type must be 'agent', 'company', 'holding', or 'manager'"})));
    }

    let ciphertext = match crypto.encrypt(p.value.as_bytes()) {
        Ok(ct) => ct,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Encryption failed: {}", e)}))),
    };

    let id = Uuid::new_v4();
    let desc = p.description.as_deref().unwrap_or("");
    match sqlx::query(
        "INSERT INTO secrets (id, scope_type, scope_id, kind, ciphertext, description) VALUES ($1,$2,$3,$4,$5,$6)"
    ).bind(id).bind(&p.scope_type).bind(p.scope_id).bind(&p.name).bind(&ciphertext).bind(desc)
    .execute(&state.db).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "name": p.name, "scope_type": p.scope_type, "scope_id": p.scope_id, "description": desc}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)}))),
    }
}

async fn list_secrets(
    State(state): State<AppState>, Query(q): Query<SecretsQuery>
) -> impl IntoResponse {
    // Return metadata only — NEVER return plaintext values
    let secrets: Vec<(Uuid, String, Uuid, String, String, chrono::DateTime<chrono::Utc>)> = if let (Some(st), Some(si)) = (&q.scope_type, &q.scope_id) {
        sqlx::query_as(
            "SELECT id, scope_type, scope_id, kind, description, created_at FROM secrets WHERE scope_type = $1 AND scope_id = $2 ORDER BY created_at DESC"
        ).bind(st).bind(si).fetch_all(&state.db).await.unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT id, scope_type, scope_id, kind, description, created_at FROM secrets ORDER BY created_at DESC"
        ).fetch_all(&state.db).await.unwrap_or_default()
    };

    let result: Vec<Value> = secrets.iter().map(|(id, st, si, kind, desc, created)| {
        json!({"id": id, "scope_type": st, "scope_id": si, "name": kind, "description": desc, "created_at": created})
    }).collect();

    (StatusCode::OK, Json(json!(result)))
}

async fn delete_secret(
    State(state): State<AppState>, Path(id): Path<String>
) -> impl IntoResponse {
    let secret_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid ID"}))),
    };
    let _ = sqlx::query("DELETE FROM secrets WHERE id = $1").bind(secret_id).execute(&state.db).await;
    (StatusCode::OK, Json(json!({"status":"deleted"})))
}

/// List all secrets accessible to an agent (names and descriptions only, never values).
/// Uses the same hierarchical scope logic as get_agent_secret.
async fn list_agent_secrets(
    State(state): State<AppState>, Path(id): Path<String>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };

    // Get agent's company_id, holding_id, parent_agent_id, and role for hierarchical lookup
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT a.company_id, a.holding_id, a.parent_agent_id, a.role FROM agents a WHERE a.id = $1"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some((cid, hid, pid, r)) => (cid, hid, pid, r),
        None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
    };

    // Build hierarchical scopes: agent → manager (department) → company → holding
    let mut scopes: Vec<(&str, Uuid)> = vec![("agent", agent_id)];
    if role == "MANAGER" {
        scopes.push(("manager", agent_id));
    }
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(&state.db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scopes.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scopes.push(("company", cid));
    }
    scopes.push(("holding", holding_id));

    // Collect all accessible secrets (dedup by name — first scope wins, matching fetch behavior)
    let mut seen_names = std::collections::HashSet::new();
    let mut result: Vec<Value> = Vec::new();

    for (scope_type, scope_id) in &scopes {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, description FROM secrets WHERE scope_type = $1 AND scope_id = $2 ORDER BY kind"
        ).bind(scope_type).bind(scope_id)
        .fetch_all(&state.db).await.unwrap_or_default();

        for (name, description) in rows {
            if seen_names.insert(name.clone()) {
                result.push(json!({
                    "name": name,
                    "description": description,
                    "scope": scope_type,
                }));
            }
        }
    }

    (StatusCode::OK, Json(json!(result)))
}

/// Agent fetches a secret by name. Performs hierarchical lookup:
/// 1. Agent-scoped secrets (scope_type='agent', scope_id=agent_id)
/// 2. Manager/department-scoped secrets (scope_type='manager', scope_id=manager_id)
/// 3. Company-scoped secrets (scope_type='company', scope_id=company_id)
/// 4. Holding-scoped secrets (scope_type='holding', scope_id=holding_id)
async fn get_agent_secret(
    State(state): State<AppState>, Path((id, name)): Path<(String, String)>
) -> impl IntoResponse {
    let agent_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid agent ID"}))),
    };
    let crypto = match &state.crypto {
        Some(c) => c,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Secrets not available"}))),
    };

    // Get agent's company_id, holding_id, parent_agent_id, and role for hierarchical lookup
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT a.company_id, a.holding_id, a.parent_agent_id, a.role FROM agents a WHERE a.id = $1"
    ).bind(agent_id).fetch_optional(&state.db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some((cid, hid, pid, r)) => (cid, hid, pid, r),
        None => return (StatusCode::NOT_FOUND, Json(json!({"error":"Agent not found"}))),
    };

    // Hierarchical lookup: agent → manager (department) → company → holding
    let mut scopes: Vec<(&str, Uuid)> = vec![("agent", agent_id)];
    // Managers can access their own department secrets
    if role == "MANAGER" {
        scopes.push(("manager", agent_id));
    }
    // Workers inherit their manager's department secrets
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(&state.db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scopes.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scopes.push(("company", cid));
    }
    scopes.push(("holding", holding_id));

    for (scope_type, scope_id) in &scopes {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT ciphertext FROM secrets WHERE scope_type = $1 AND scope_id = $2 AND kind = $3 LIMIT 1"
        ).bind(scope_type).bind(scope_id).bind(&name)
        .fetch_optional(&state.db).await.ok().flatten();

        if let Some((ciphertext,)) = row {
            match crypto.decrypt(&ciphertext) {
                Ok(plaintext) => {
                    let value = String::from_utf8_lossy(&plaintext).to_string();
                    return (StatusCode::OK, Json(json!({"name": name, "value": value})));
                }
                Err(e) => {
                    tracing::error!("Failed to decrypt secret '{}': {}", name, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"Decryption failed"})));
                }
            }
        }
    }

    (StatusCode::NOT_FOUND, Json(json!({"error": format!("Secret '{}' not found", name)})))
}

/// Scrub known secret values from agent message text to prevent leaks.
async fn scrub_secrets(db: &sqlx::PgPool, crypto: &crate::crypto::CryptoMaster, agent_id: Uuid, text: &str) -> String {
    // Get agent's company_id, holding_id, parent_agent_id, and role
    let agent_info: Option<(Option<Uuid>, Uuid, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT company_id, holding_id, parent_agent_id, role FROM agents WHERE id = $1"
    ).bind(agent_id).fetch_optional(db).await.ok().flatten();

    let (company_id, holding_id, parent_agent_id, role) = match agent_info {
        Some(info) => info,
        None => return text.to_string(),
    };

    // Collect all scope IDs to query (agent → manager → company → holding)
    let mut scope_conditions = vec![("agent", agent_id)];
    // Manager's own department secrets
    if role == "MANAGER" {
        scope_conditions.push(("manager", agent_id));
    }
    // Worker's department secrets (parent is a manager)
    if let Some(pid) = parent_agent_id {
        let parent_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM agents WHERE id = $1"
        ).bind(pid).fetch_optional(db).await.ok().flatten();
        if parent_role.as_deref() == Some("MANAGER") {
            scope_conditions.push(("manager", pid));
        }
    }
    if let Some(cid) = company_id {
        scope_conditions.push(("company", cid));
    }
    scope_conditions.push(("holding", holding_id));

    let mut scrubbed = text.to_string();
    for (scope_type, scope_id) in &scope_conditions {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT ciphertext FROM secrets WHERE scope_type = $1 AND scope_id = $2"
        ).bind(scope_type).bind(scope_id)
        .fetch_all(db).await.unwrap_or_default();

        for (ciphertext,) in rows {
            if let Ok(plaintext) = crypto.decrypt(&ciphertext) {
                if let Ok(secret_str) = String::from_utf8(plaintext) {
                    if secret_str.len() >= 4 && scrubbed.contains(&secret_str) {
                        scrubbed = scrubbed.replace(&secret_str, "[REDACTED]");
                    }
                }
            }
        }
    }

    scrubbed
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

// ═══════════════════════════════════════════════════════════════
// World Snapshot
// ═══════════════════════════════════════════════════════════════

/// Aggregated snapshot endpoint for the 3D world view.
/// Returns companies, agents, balances, activities, and VM states in one call.
async fn world_snapshot(State(state): State<AppState>) -> impl IntoResponse {
    // 1. Fetch all companies
    let companies: Vec<Company> = sqlx::query_as(
        "SELECT id, holding_id, name, type, description, tags, status, created_at FROM companies ORDER BY created_at"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // 2. Fetch all agents (excluding MAIN role — they're not in any company)
    let agents: Vec<Agent> = sqlx::query_as(
        "SELECT id, holding_id, company_id, role, name, specialty, parent_agent_id, preferred_model, effective_model, system_prompt, tool_policy_id, vm_id, sandbox_vm_id, handle, status, created_at \
         FROM agents WHERE role != 'MAIN' ORDER BY created_at"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // 3. Fetch balances for all companies in one query
    let balance_rows: Vec<(Uuid, String, String, rust_decimal::Decimal)> = sqlx::query_as(
        "SELECT company_id, currency, type, COALESCE(SUM(amount), 0) as total \
         FROM ledger_entries GROUP BY company_id, currency, type"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let mut balances: serde_json::Map<String, Value> = serde_json::Map::new();
    for (company_id, currency, entry_type, total) in &balance_rows {
        let company_key = company_id.to_string();
        let company_obj = balances.entry(company_key)
            .or_insert_with(|| json!({}));
        let currency_obj = company_obj.as_object_mut().unwrap()
            .entry(currency.clone())
            .or_insert_with(|| json!({"revenue": 0.0, "expenses": 0.0, "capital": 0.0, "net": 0.0}));
        let total_f64 = total.to_string().parse::<f64>().unwrap_or(0.0);
        match entry_type.as_str() {
            "REVENUE" => { currency_obj["revenue"] = json!(total_f64); }
            "EXPENSE" => { currency_obj["expenses"] = json!(total_f64); }
            "CAPITAL_INJECTION" => { currency_obj["capital"] = json!(total_f64); }
            "INTERNAL_TRANSFER" => { currency_obj["expenses"] = json!(currency_obj["expenses"].as_f64().unwrap_or(0.0) + total_f64); }
            _ => {}
        }
    }
    // Calculate net for each company/currency
    for (_, company_obj) in balances.iter_mut() {
        if let Some(currencies) = company_obj.as_object_mut() {
            for (_, obj) in currencies.iter_mut() {
                let revenue = obj["revenue"].as_f64().unwrap_or(0.0);
                let expenses = obj["expenses"].as_f64().unwrap_or(0.0);
                let capital = obj["capital"].as_f64().unwrap_or(0.0);
                obj["net"] = json!(capital + revenue - expenses);
            }
        }
    }

    // 4. Activities — from in-memory tracker (if present), otherwise empty
    let activities: serde_json::Map<String, Value> = if let Some(ref tracker) = *state.agent_activities.read().await {
        let mut map = serde_json::Map::new();
        for (agent_id, activity) in tracker.iter() {
            map.insert(agent_id.to_string(), json!({
                "agent_id": agent_id.to_string(),
                "status": activity.status,
                "task": activity.task,
                "since": activity.since,
            }));
        }
        map
    } else {
        serde_json::Map::new()
    };

    // 5. VM states — check if agents have VMs provisioned, batch-query status
    let mut vm_states: serde_json::Map<String, Value> = serde_json::Map::new();

    // Query which agents have VMs assigned
    let vm_rows: Vec<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT a.id, v_desktop.provider_ref, v_sandbox.provider_ref \
         FROM agents a \
         LEFT JOIN vms v_desktop ON a.vm_id = v_desktop.id \
         LEFT JOIN vms v_sandbox ON a.sandbox_vm_id = v_sandbox.id \
         WHERE a.role != 'MAIN'"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // If we have a VM provider, batch-query running instances
    let running_vms: std::collections::HashSet<String> = if let Some(ref provider) = state.vm_provider {
        // Get all running instances in one call
        match provider.list_running().await {
            Ok(names) => names.into_iter().collect(),
            Err(_) => std::collections::HashSet::new(),
        }
    } else {
        std::collections::HashSet::new()
    };

    for (agent_id, desktop_ref, sandbox_ref) in &vm_rows {
        let desktop_status = match desktop_ref {
            Some(name) if running_vms.contains(name) => "RUNNING",
            Some(_) => "STOPPED",
            None => "UNKNOWN",
        };
        let sandbox_status = match sandbox_ref {
            Some(name) if running_vms.contains(name) => "RUNNING",
            Some(_) => "STOPPED",
            None => "UNKNOWN",
        };
        vm_states.insert(agent_id.to_string(), json!({
            "desktop": desktop_status,
            "sandbox": sandbox_status,
        }));
    }

    (StatusCode::OK, Json(json!({
        "companies": companies,
        "agents": agents,
        "balances": balances,
        "activities": activities,
        "vm_states": vm_states,
    })))
}
