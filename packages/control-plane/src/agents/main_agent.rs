use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// The Main Agent runs in-process inside multiclawd, not inside a VM.
/// It uses Ollama's /api/chat with function calling to create companies,
/// approve requests, hire staff, and manage the holding company.

#[derive(Debug, Clone)]
pub struct MainAgent {
    pub name: String,
    model: String,
    ollama_url: String,
    client: Client,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Value>,
    tools: Vec<Value>,
    stream: bool,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    message: MessageObj,
}

#[derive(Deserialize, Serialize, Debug)]
struct MessageObj {
    role: String,
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<Value>>,
}

impl MainAgent {
    pub fn new(name: String, model: String, ollama_url: String) -> Self {
        Self {
            name,
            model,
            ollama_url,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Primary loop: handles a user message, calls Ollama, executes tools, returns response.
    pub async fn handle_message(&self, db_pool: &PgPool, user_content: &str) -> Result<String> {
        let tools = self.get_tools();
        let mut messages = vec![
            json!({
                "role": "system",
                "content": format!(
                    "You are {}, the Main Agent of an AI holding company called MultiClaw. \
                     You manage companies, approve/reject requests, hire CEOs/managers/workers, \
                     and oversee all operations. You can use the provided tools to take actions. \
                     Be concise, decisive, and proactive. When asked to create a company, \
                     always hire a CEO for it immediately after creation. \
                     When reporting results, be specific about what you did (include names, IDs).",
                    self.name
                )
            }),
            json!({
                "role": "user",
                "content": user_content
            })
        ];

        // Retry loop for tool calls (max 5 rounds)
        for _round in 0..5 {
            let req = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                stream: false,
            };

            tracing::debug!("Calling Ollama: model={}, messages={}", self.model, messages.len());

            let res = self.client
                .post(&format!("{}/api/chat", self.ollama_url))
                .json(&req)
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() {
                let body = res.text().await.unwrap_or_default();
                tracing::error!("Ollama returned {}: {}", status, body);
                return Ok(format!("I'm having trouble connecting to my language model (status {}). Please check that Ollama is running.", status));
            }

            let chat_res: ChatResponse = res.json().await?;

            if let Some(ref calls) = chat_res.message.tool_calls {
                if !calls.is_empty() {
                    // Add assistant message with tool calls
                    messages.push(serde_json::to_value(&chat_res.message)?);

                    // Execute each tool call
                    for call in calls {
                        let func_name = call["function"]["name"].as_str().unwrap_or("");
                        let args = call["function"]["arguments"].as_object();

                        tracing::info!("MainAgent calling tool: {} with args: {:?}", func_name, args);
                        let tool_result = self.execute_tool(db_pool, func_name, args).await;
                        tracing::info!("Tool result: {}", tool_result);

                        messages.push(json!({
                            "role": "tool",
                            "content": tool_result
                        }));
                    }

                    // Continue loop to let LLM process tool results
                    continue;
                }
            }

            // No tool calls — return the text response
            let content = chat_res.message.content.trim().to_string();
            if content.is_empty() {
                return Ok("I processed your request.".to_string());
            }
            return Ok(content);
        }

        Ok("I completed processing your request after multiple steps.".to_string())
    }

    fn get_tools(&self) -> Vec<Value> {
        vec![
            json!({
                "type": "function",
                "function": {
                    "name": "create_company",
                    "description": "Create a new company under the holding. After creating, you should hire a CEO for it.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Company name" },
                            "company_type": { "type": "string", "enum": ["INTERNAL", "EXTERNAL"], "description": "Type of company" },
                            "description": { "type": "string", "description": "What this company does / its purpose" }
                        },
                        "required": ["name", "company_type", "description"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "hire_ceo",
                    "description": "Hire a CEO for a company. Use the company_id from create_company.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "company_id": { "type": "string", "description": "UUID of the company" },
                            "name": { "type": "string", "description": "Name for the CEO agent" },
                            "specialty": { "type": "string", "description": "CEO's specialty/focus area" }
                        },
                        "required": ["company_id", "name"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "hire_manager",
                    "description": "Hire a manager for a company. Managers report to the CEO.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "company_id": { "type": "string", "description": "UUID of the company" },
                            "name": { "type": "string", "description": "Name for the manager agent" },
                            "specialty": { "type": "string", "description": "Manager's department/focus" }
                        },
                        "required": ["company_id", "name"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "hire_worker",
                    "description": "Hire a worker for a company. Workers report to managers.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "company_id": { "type": "string", "description": "UUID of the company" },
                            "name": { "type": "string", "description": "Name for the worker agent" },
                            "specialty": { "type": "string", "description": "Worker's skill/task area" }
                        },
                        "required": ["company_id", "name"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "list_companies",
                    "description": "List all companies in the holding.",
                    "parameters": { "type": "object", "properties": {} }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "list_agents",
                    "description": "List all agents across the holding.",
                    "parameters": { "type": "object", "properties": {} }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "list_pending_requests",
                    "description": "List all requests waiting for approval.",
                    "parameters": { "type": "object", "properties": {} }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "approve_request",
                    "description": "Approve a pending request by its ID.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "request_id": { "type": "string", "description": "UUID of the request" }
                        },
                        "required": ["request_id"]
                    }
                }
            }),
        ]
    }

    async fn execute_tool(
        &self,
        db_pool: &PgPool,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
    ) -> String {
        let args = match args {
            Some(a) => a,
            None => return "Error: no arguments provided".to_string(),
        };

        match name {
            "create_company" => self.tool_create_company(db_pool, args).await,
            "hire_ceo" => self.tool_hire_agent(db_pool, args, "CEO").await,
            "hire_manager" => self.tool_hire_agent(db_pool, args, "MANAGER").await,
            "hire_worker" => self.tool_hire_agent(db_pool, args, "WORKER").await,
            "list_companies" => self.tool_list_companies(db_pool).await,
            "list_agents" => self.tool_list_agents(db_pool).await,
            "list_pending_requests" => self.tool_list_pending_requests(db_pool).await,
            "approve_request" => self.tool_approve_request(db_pool, args).await,
            _ => format!("Unknown tool: {}", name),
        }
    }

    async fn tool_create_company(
        &self,
        db_pool: &PgPool,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("New Company");
        let company_type = args.get("company_type").and_then(|v| v.as_str()).unwrap_or("INTERNAL");
        let description = args.get("description").and_then(|v| v.as_str());
        let id = Uuid::new_v4();

        // Get holding_id
        let holding_id: Uuid = sqlx::query_scalar("SELECT id FROM holdings LIMIT 1")
            .fetch_optional(db_pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(Uuid::from_u128(0));

        match sqlx::query(
            "INSERT INTO companies (id, holding_id, name, type, description, status) VALUES ($1,$2,$3,$4,$5,'ACTIVE')"
        )
        .bind(id)
        .bind(holding_id)
        .bind(name)
        .bind(company_type)
        .bind(description)
        .execute(db_pool)
        .await
        {
            Ok(_) => format!("Company '{}' created successfully with ID: {}. You should now hire a CEO for this company.", name, id),
            Err(e) => format!("Failed to create company: {}", e),
        }
    }

    async fn tool_hire_agent(
        &self,
        db_pool: &PgPool,
        args: &serde_json::Map<String, Value>,
        role: &str,
    ) -> String {
        let company_id_str = args.get("company_id").and_then(|v| v.as_str()).unwrap_or("");
        let company_id = match Uuid::parse_str(company_id_str) {
            Ok(u) => u,
            Err(_) => return format!("Invalid company_id: '{}'", company_id_str),
        };
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("Agent");
        let specialty = args.get("specialty").and_then(|v| v.as_str());

        // Get holding_id
        let holding_id: Uuid = sqlx::query_scalar("SELECT id FROM holdings LIMIT 1")
            .fetch_optional(db_pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(Uuid::from_u128(0));

        // Get the policy for this role
        let policy_name = match role {
            "CEO" => "ceo_policy",
            "MANAGER" => "manager_policy",
            _ => "worker_policy",
        };
        let policy_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM tool_policies WHERE name = $1 LIMIT 1"
        )
        .bind(policy_name)
        .fetch_optional(db_pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(Uuid::new_v4());

        // Get the default model from the MainAgent's own model
        let model = &self.model;

        // Get parent agent (CEO for managers/workers, MainAgent for CEO)
        let parent_agent_id: Option<Uuid> = match role {
            "CEO" => {
                sqlx::query_scalar("SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1")
                    .fetch_optional(db_pool).await.ok().flatten()
            }
            "MANAGER" => {
                sqlx::query_scalar("SELECT id FROM agents WHERE company_id = $1 AND role = 'CEO' LIMIT 1")
                    .bind(company_id)
                    .fetch_optional(db_pool).await.ok().flatten()
            }
            _ => {
                sqlx::query_scalar("SELECT id FROM agents WHERE company_id = $1 AND role = 'MANAGER' LIMIT 1")
                    .bind(company_id)
                    .fetch_optional(db_pool).await.ok().flatten()
            }
        };

        // Get company description for system prompt
        let company_desc: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(description, name) FROM companies WHERE id = $1"
        )
        .bind(company_id)
        .fetch_optional(db_pool)
        .await
        .ok()
        .flatten();

        let system_prompt = format!(
            "You are {}, a {} at a company focused on: {}. You work autonomously within the MultiClaw holding company system.",
            name, role, company_desc.as_deref().unwrap_or("general operations")
        );

        let agent_id = Uuid::new_v4();
        match sqlx::query(
            "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, parent_agent_id, effective_model, system_prompt, tool_policy_id, status) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'ACTIVE')"
        )
        .bind(agent_id)
        .bind(holding_id)
        .bind(company_id)
        .bind(role)
        .bind(name)
        .bind(specialty)
        .bind(parent_agent_id)
        .bind(model)
        .bind(&system_prompt)
        .bind(policy_id)
        .execute(db_pool)
        .await
        {
            Ok(_) => {
                // If CEO, also add to company_ceos table
                if role == "CEO" {
                    let _ = sqlx::query("INSERT INTO company_ceos (company_id, agent_id) VALUES ($1, $2)")
                        .bind(company_id).bind(agent_id)
                        .execute(db_pool).await;
                }
                format!("{} '{}' hired successfully for company {} with ID: {}", role, name, company_id, agent_id)
            }
            Err(e) => format!("Failed to hire {}: {}", role, e),
        }
    }

    async fn tool_list_companies(&self, db_pool: &PgPool) -> String {
        match sqlx::query_as::<_, (Uuid, String, String, Option<String>)>(
            "SELECT id, name, type, description FROM companies ORDER BY created_at DESC LIMIT 20"
        )
        .fetch_all(db_pool)
        .await
        {
            Ok(rows) => {
                if rows.is_empty() {
                    return "No companies found.".to_string();
                }
                let items: Vec<String> = rows.iter().map(|(id, name, t, desc)| {
                    format!("- {} (type: {}, id: {}){}", name, t, id,
                        desc.as_ref().map(|d| format!(" — {}", d)).unwrap_or_default())
                }).collect();
                format!("Companies:\n{}", items.join("\n"))
            }
            Err(e) => format!("Error listing companies: {}", e),
        }
    }

    async fn tool_list_agents(&self, db_pool: &PgPool) -> String {
        match sqlx::query_as::<_, (Uuid, String, String, Option<Uuid>)>(
            "SELECT id, name, role, company_id FROM agents ORDER BY created_at DESC LIMIT 30"
        )
        .fetch_all(db_pool)
        .await
        {
            Ok(rows) => {
                if rows.is_empty() {
                    return "No agents found.".to_string();
                }
                let items: Vec<String> = rows.iter().map(|(id, name, role, cid)| {
                    format!("- {} (role: {}, id: {}, company: {})", name, role, id,
                        cid.map(|c| c.to_string()).unwrap_or_else(|| "N/A".into()))
                }).collect();
                format!("Agents:\n{}", items.join("\n"))
            }
            Err(e) => format!("Error listing agents: {}", e),
        }
    }

    async fn tool_list_pending_requests(&self, db_pool: &PgPool) -> String {
        match sqlx::query_as::<_, (Uuid, String, Value)>(
            "SELECT id, type, payload FROM requests WHERE status = 'PENDING' ORDER BY created_at DESC LIMIT 20"
        )
        .fetch_all(db_pool)
        .await
        {
            Ok(rows) => {
                if rows.is_empty() {
                    return "No pending requests.".to_string();
                }
                let items: Vec<String> = rows.iter().map(|(id, t, payload)| {
                    format!("- Request {} (type: {}): {}", id, t, payload)
                }).collect();
                format!("Pending requests:\n{}", items.join("\n"))
            }
            Err(e) => format!("Error listing requests: {}", e),
        }
    }

    async fn tool_approve_request(
        &self,
        db_pool: &PgPool,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let request_id_str = args.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = match Uuid::parse_str(request_id_str) {
            Ok(u) => u,
            Err(_) => return format!("Invalid request_id: '{}'", request_id_str),
        };

        let approval_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO approvals (id, request_id, approver_type, approver_id, decision) VALUES ($1,$2,'AGENT',$3,'APPROVE')"
        )
        .bind(approval_id)
        .bind(request_id)
        .bind(Uuid::new_v4()) // MainAgent's virtual approver ID
        .execute(db_pool)
        .await;

        match sqlx::query("UPDATE requests SET status = 'APPROVED', updated_at = NOW() WHERE id = $1")
            .bind(request_id)
            .execute(db_pool)
            .await
        {
            Ok(_) => format!("Request {} approved successfully.", request_id),
            Err(e) => format!("Failed to approve request: {}", e),
        }
    }
}
