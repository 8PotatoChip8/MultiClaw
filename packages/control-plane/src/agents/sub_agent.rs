use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// A SubAgent handles chat for any non-MAIN agent (CEO, Manager, Worker).
/// It reads the agent's identity from the DB and calls Ollama with
/// that agent's own system prompt, name, and model.

#[derive(Debug, Clone)]
pub struct SubAgent {
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

impl SubAgent {
    pub fn new(ollama_url: String) -> Self {
        Self {
            ollama_url,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Handle a message on behalf of a specific agent (looked up by ID).
    pub async fn handle_message(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        user_content: &str,
    ) -> Result<String> {
        // Fetch agent info from DB
        let agent_row: Option<(String, String, String, Option<String>, Option<Uuid>)> =
            sqlx::query_as(
                "SELECT name, role, effective_model, system_prompt, company_id \
                 FROM agents WHERE id = $1",
            )
            .bind(agent_id)
            .fetch_optional(db_pool)
            .await?;

        let (agent_name, role, model, system_prompt, company_id) = match agent_row {
            Some(r) => r,
            None => return Ok("Error: agent not found".to_string()),
        };

        // Get company name & description for context
        let company_info: Option<(String, Option<String>)> = if let Some(cid) = company_id {
            sqlx::query_as("SELECT name, description FROM companies WHERE id = $1")
                .bind(cid)
                .fetch_optional(db_pool)
                .await
                .ok()
                .flatten()
        } else {
            None
        };

        let (company_name, company_desc) = company_info
            .unwrap_or_else(|| ("the company".to_string(), None));

        // Get holding name
        let holding_name: String =
            sqlx::query_scalar("SELECT name FROM holdings LIMIT 1")
                .fetch_optional(db_pool)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "the holding company".into());

        // Fetch agent memories
        let mut memory_section = String::new();
        let memories: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT category, key, content FROM agent_memories \
             WHERE agent_id = $1 ORDER BY importance DESC, updated_at DESC LIMIT 20",
        )
        .bind(agent_id)
        .fetch_all(db_pool)
        .await
        .unwrap_or_default();

        if !memories.is_empty() {
            memory_section.push_str("\n\nYour memories (use these to maintain context):\n");
            for (cat, key, content) in &memories {
                memory_section.push_str(&format!("- [{}] {}: {}\n", cat, key, content));
            }
        }

        // Build system prompt — use DB system_prompt if available, otherwise generate one
        let full_system_prompt = if let Some(ref sp) = system_prompt {
            format!(
                "{}\n\nYou work at '{}' ({}), which is part of the '{}' holding company. \
                 You run on the OpenClaw runtime. Be concise, helpful, and proactive. \
                 Use save_memory to remember important facts and recall_memories to look things up.{}",
                sp,
                company_name,
                company_desc.as_deref().unwrap_or("general operations"),
                holding_name,
                memory_section
            )
        } else {
            format!(
                "You are {}, a {} at '{}' ({}), part of the '{}' holding company. \
                 You run on the OpenClaw runtime, an AI agent platform. \
                 Be concise, helpful, and proactive. \
                 Use save_memory to remember important facts and recall_memories to look things up.{}",
                agent_name,
                role,
                company_name,
                company_desc.as_deref().unwrap_or("general operations"),
                holding_name,
                memory_section
            )
        };

        let tools = self.get_tools_for_role(&role);

        let mut messages = vec![
            json!({
                "role": "system",
                "content": full_system_prompt
            }),
            json!({
                "role": "user",
                "content": user_content
            }),
        ];

        // Retry loop for tool calls (max 5 rounds)
        for _round in 0..5 {
            let req = ChatRequest {
                model: model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                stream: false,
            };

            tracing::debug!(
                "SubAgent {} calling Ollama: model={}, messages={}",
                agent_name,
                model,
                messages.len()
            );

            let res = self
                .client
                .post(&format!("{}/api/chat", self.ollama_url))
                .json(&req)
                .send()
                .await?;

            let status = res.status();
            if !status.is_success() {
                let body = res.text().await.unwrap_or_default();
                tracing::error!("Ollama returned {}: {}", status, body);
                return Ok(format!(
                    "I'm having trouble connecting to my language model (status {}). Please check that Ollama is running.",
                    status
                ));
            }

            let chat_res: ChatResponse = res.json().await?;

            if let Some(ref calls) = chat_res.message.tool_calls {
                if !calls.is_empty() {
                    messages.push(serde_json::to_value(&chat_res.message)?);

                    for call in calls {
                        let func_name = call["function"]["name"].as_str().unwrap_or("");
                        let args = call["function"]["arguments"].as_object();

                        tracing::info!(
                            "{} ({}) calling tool: {} with args: {:?}",
                            agent_name,
                            role,
                            func_name,
                            args
                        );
                        let tool_result =
                            self.execute_tool(db_pool, agent_id, func_name, args).await;
                        tracing::info!("Tool result: {}", tool_result);

                        messages.push(json!({
                            "role": "tool",
                            "content": tool_result
                        }));
                    }
                    continue;
                }
            }

            let content = chat_res.message.content.trim().to_string();
            if content.is_empty() {
                return Ok("I processed your request.".to_string());
            }
            return Ok(content);
        }

        Ok("I completed processing your request after multiple steps.".to_string())
    }

    /// Return tools appropriate for the agent's role.
    fn get_tools_for_role(&self, role: &str) -> Vec<Value> {
        let mut tools = vec![
            // All agents can save/recall/forget memories
            json!({
                "type": "function",
                "function": {
                    "name": "save_memory",
                    "description": "Save an important piece of information to your long-term memory.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "category": { "type": "string", "enum": ["IDENTITY","TASK","CONTEXT","NOTE"], "description": "Memory category" },
                            "key": { "type": "string", "description": "Short key/label for this memory" },
                            "content": { "type": "string", "description": "The content to remember" },
                            "importance": { "type": "integer", "description": "1-10 importance level" }
                        },
                        "required": ["category", "key", "content"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "recall_memories",
                    "description": "Search your memories for information matching a keyword or topic.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Keyword or topic to search for" }
                        },
                        "required": ["query"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "forget_memory",
                    "description": "Remove a memory by its key.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "key": { "type": "string", "description": "Key of the memory to forget" }
                        },
                        "required": ["key"]
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "list_agents",
                    "description": "List all agents in your company or the holding.",
                    "parameters": { "type": "object", "properties": {} }
                }
            }),
        ];

        // CEOs can hire managers and workers
        if role == "CEO" || role == "MANAGER" {
            tools.push(json!({
                "type": "function",
                "function": {
                    "name": "hire_worker",
                    "description": "Hire a worker for your company.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Name for the worker" },
                            "specialty": { "type": "string", "description": "Worker's skill/task area" }
                        },
                        "required": ["name"]
                    }
                }
            }));
        }

        if role == "CEO" {
            tools.push(json!({
                "type": "function",
                "function": {
                    "name": "hire_manager",
                    "description": "Hire a manager for your company.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Name for the manager" },
                            "specialty": { "type": "string", "description": "Manager's department/focus" }
                        },
                        "required": ["name"]
                    }
                }
            }));
        }

        // All agents can submit requests to their superior
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "submit_request",
                "description": "Submit a request to your superior for approval (e.g. budget, resources, actions).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "request_type": { "type": "string", "description": "Type of request (e.g. BUDGET, RESOURCE, ACTION)" },
                        "description": { "type": "string", "description": "What you are requesting and why" }
                    },
                    "required": ["request_type", "description"]
                }
            }
        }));

        tools
    }

    async fn execute_tool(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
    ) -> String {
        let empty = serde_json::Map::new();
        let args = args.unwrap_or(&empty);

        match name {
            "save_memory" => self.tool_save_memory(db_pool, agent_id, args).await,
            "recall_memories" => self.tool_recall_memories(db_pool, agent_id, args).await,
            "forget_memory" => self.tool_forget_memory(db_pool, agent_id, args).await,
            "list_agents" => self.tool_list_agents(db_pool, agent_id).await,
            "hire_worker" => self.tool_hire(db_pool, agent_id, args, "WORKER").await,
            "hire_manager" => self.tool_hire(db_pool, agent_id, args, "MANAGER").await,
            "submit_request" => self.tool_submit_request(db_pool, agent_id, args).await,
            _ => format!("Unknown tool: {}", name),
        }
    }

    async fn tool_save_memory(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("NOTE");
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let importance = args.get("importance").and_then(|v| v.as_i64()).unwrap_or(5) as i32;

        if key.is_empty() || content.is_empty() {
            return "Error: key and content are required".to_string();
        }

        let mem_id = Uuid::new_v4();
        match sqlx::query(
            "INSERT INTO agent_memories (id, agent_id, category, key, content, importance) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (agent_id, category, key) DO UPDATE SET content = $5, importance = $6, updated_at = NOW()",
        )
        .bind(mem_id)
        .bind(agent_id)
        .bind(category)
        .bind(key)
        .bind(content)
        .bind(importance)
        .execute(db_pool)
        .await
        {
            Ok(_) => format!("Memory saved: [{}] {} (importance: {})", category, key, importance),
            Err(e) => format!("Failed to save memory: {}", e),
        }
    }

    async fn tool_recall_memories(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let pattern = format!("%{}%", query);
        let memories: Vec<(String, String, String, i32)> = sqlx::query_as(
            "SELECT category, key, content, importance FROM agent_memories \
             WHERE agent_id = $1 AND (key ILIKE $2 OR content ILIKE $2) \
             ORDER BY importance DESC, updated_at DESC LIMIT 10",
        )
        .bind(agent_id)
        .bind(&pattern)
        .fetch_all(db_pool)
        .await
        .unwrap_or_default();

        if memories.is_empty() {
            return format!("No memories found matching '{}'", query);
        }

        let mut result = format!("Found {} memories matching '{}':\n", memories.len(), query);
        for (cat, key, content, imp) in &memories {
            result.push_str(&format!("- [{}] {} (importance: {}): {}\n", cat, key, imp, content));
        }
        result
    }

    async fn tool_forget_memory(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
        match sqlx::query("DELETE FROM agent_memories WHERE agent_id = $1 AND key = $2")
            .bind(agent_id)
            .bind(key)
            .execute(db_pool)
            .await
        {
            Ok(r) => {
                if r.rows_affected() > 0 {
                    format!("Memory '{}' forgotten.", key)
                } else {
                    format!("No memory found with key '{}'", key)
                }
            }
            Err(e) => format!("Failed to forget memory: {}", e),
        }
    }

    async fn tool_list_agents(&self, db_pool: &PgPool, agent_id: Uuid) -> String {
        // Get agent's company_id
        let company_id: Option<Uuid> =
            sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1")
                .bind(agent_id)
                .fetch_optional(db_pool)
                .await
                .ok()
                .flatten();

        let rows: Vec<(Uuid, String, String, Option<String>)> = if let Some(cid) = company_id {
            sqlx::query_as(
                "SELECT id, name, role, specialty FROM agents WHERE company_id = $1 ORDER BY role, name",
            )
            .bind(cid)
            .fetch_all(db_pool)
            .await
            .unwrap_or_default()
        } else {
            sqlx::query_as(
                "SELECT id, name, role, specialty FROM agents ORDER BY role, name LIMIT 20",
            )
            .fetch_all(db_pool)
            .await
            .unwrap_or_default()
        };

        if rows.is_empty() {
            return "No agents found.".to_string();
        }
        let items: Vec<String> = rows
            .iter()
            .map(|(id, name, role, spec)| {
                format!(
                    "- {} (role: {}, id: {}){}",
                    name,
                    role,
                    id,
                    spec.as_ref()
                        .map(|s| format!(" — {}", s))
                        .unwrap_or_default()
                )
            })
            .collect();
        format!("Agents:\n{}", items.join("\n"))
    }

    async fn tool_hire(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        args: &serde_json::Map<String, Value>,
        role: &str,
    ) -> String {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("Agent");
        let specialty = args.get("specialty").and_then(|v| v.as_str());

        // Get agent's company_id and model
        let agent_info: Option<(Option<Uuid>, Uuid, String)> = sqlx::query_as(
            "SELECT company_id, holding_id, effective_model FROM agents WHERE id = $1",
        )
        .bind(agent_id)
        .fetch_optional(db_pool)
        .await
        .ok()
        .flatten();

        let (company_id, holding_id, model) = match agent_info {
            Some((Some(cid), hid, m)) => (cid, hid, m),
            _ => return "Error: could not determine your company".to_string(),
        };

        // Get policy
        let policy_name = if role == "MANAGER" {
            "manager_policy"
        } else {
            "worker_policy"
        };
        let policy_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM tool_policies WHERE name = $1 LIMIT 1",
        )
        .bind(policy_name)
        .fetch_optional(db_pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(Uuid::new_v4());

        // Company name for handle generation
        let company_name: String = sqlx::query_scalar("SELECT name FROM companies WHERE id = $1")
            .bind(company_id)
            .fetch_optional(db_pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "co".into());

        // Company description for system prompt
        let company_desc: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(description, name) FROM companies WHERE id = $1",
        )
        .bind(company_id)
        .fetch_optional(db_pool)
        .await
        .ok()
        .flatten();

        let system_prompt = format!(
            "You are {}, a {} at '{}' focused on: {}. You work autonomously within the MultiClaw holding company system.",
            name,
            role,
            company_name,
            company_desc.as_deref().unwrap_or("general operations")
        );

        let new_id = Uuid::new_v4();
        let slug = company_name
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        let handle = format!("@{}-{}", role.to_lowercase(), slug);

        // Check handle uniqueness
        let handle_exists: Option<(i64,)> =
            sqlx::query_as("SELECT COUNT(*) FROM agents WHERE handle = $1")
                .bind(&handle)
                .fetch_optional(db_pool)
                .await
                .ok()
                .flatten();
        let final_handle = if handle_exists.map(|h| h.0).unwrap_or(0) > 0 {
            format!("{}-{}", handle, &new_id.to_string()[..4])
        } else {
            handle
        };

        match sqlx::query(
            "INSERT INTO agents (id, holding_id, company_id, role, name, specialty, parent_agent_id, effective_model, system_prompt, tool_policy_id, handle, status) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'ACTIVE')",
        )
        .bind(new_id)
        .bind(holding_id)
        .bind(company_id)
        .bind(role)
        .bind(name)
        .bind(specialty)
        .bind(agent_id) // parent is the hiring agent
        .bind(&model)
        .bind(&system_prompt)
        .bind(policy_id)
        .bind(&final_handle)
        .execute(db_pool)
        .await
        {
            Ok(_) => format!(
                "{} '{}' (handle: {}) hired successfully with ID: {}",
                role, name, final_handle, new_id
            ),
            Err(e) => format!("Failed to hire {}: {}", role, e),
        }
    }

    async fn tool_submit_request(
        &self,
        db_pool: &PgPool,
        agent_id: Uuid,
        args: &serde_json::Map<String, Value>,
    ) -> String {
        let request_type = args
            .get("request_type")
            .and_then(|v| v.as_str())
            .unwrap_or("ACTION");
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if description.is_empty() {
            return "Error: description is required".to_string();
        }

        // Get parent agent
        let parent_id: Option<Uuid> =
            sqlx::query_scalar("SELECT parent_agent_id FROM agents WHERE id = $1")
                .bind(agent_id)
                .fetch_optional(db_pool)
                .await
                .ok()
                .flatten();

        let approver = match parent_id {
            Some(pid) => pid,
            None => {
                // Fall back to MainAgent
                sqlx::query_scalar("SELECT id FROM agents WHERE role = 'MAIN' LIMIT 1")
                    .fetch_optional(db_pool)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(Uuid::new_v4())
            }
        };

        let req_id = Uuid::new_v4();
        let payload = json!({
            "description": description,
            "requester_id": agent_id,
            "approver_id": approver
        });

        match sqlx::query(
            "INSERT INTO requests (id, type, requester_id, approver_id, payload, status) \
             VALUES ($1, $2, $3, $4, $5, 'PENDING')",
        )
        .bind(req_id)
        .bind(request_type)
        .bind(agent_id)
        .bind(approver)
        .bind(&payload)
        .execute(db_pool)
        .await
        {
            Ok(_) => format!(
                "Request submitted (ID: {}). Awaiting approval from your superior.",
                req_id
            ),
            Err(e) => format!("Failed to submit request: {}", e),
        }
    }
}
