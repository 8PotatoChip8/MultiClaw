use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// The Main Agent runs in-process inside multiclawd, not inside a VM.
// It uses Ollama's /api/chat directly (or via host proxy) with function calling
// to create companies, approve requests, and hire CEOs.

#[derive(Debug, Clone)]
pub struct MainAgent {
    pub name: String,
    model: String,
    ollama_url: String, // e.g. http://127.0.0.1:11434
    client: Client,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Value>,
    tools: Vec<Value>,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: MessageObj,
}

#[derive(Deserialize, Serialize)]
struct MessageObj {
    role: String,
    content: String,
    tool_calls: Option<Vec<Value>>,
}

impl MainAgent {
    pub fn new(name: String, model: String, ollama_url: String) -> Self {
        Self {
            name,
            model,
            ollama_url,
            client: Client::new(),
        }
    }

    /// Primary loop that handles a user message, calls Ollama, executes tools, and returns the response.
    pub async fn handle_message(&self, db_pool: &sqlx::PgPool, user_content: &str) -> Result<String> {
        let tools = self.get_tools();
        let mut messages = vec![
            serde_json::json!({
                "role": "system",
                "content": format!("You are {}, the Main Agent of this holding company. You manage companies, approve/reject requests (like headcount increases), and oversee operations. Keep your responses concise.", self.name)
            }),
            serde_json::json!({
                "role": "user",
                "content": user_content
            })
        ];

        let req = ChatRequest {
            model: self.model.clone(),
            messages: messages.clone(),
            tools,
            stream: false, // For simplicity in MVP, MainAgent is blocking
        };

        let mut res = self.client.post(&format!("{}/api/chat", self.ollama_url))
            .json(&req)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;

        // If tools called, handle them
        if let Some(calls) = &res.message.tool_calls {
            messages.push(serde_json::to_value(&res.message)?);
            
            for call in calls {
                let func_name = call["function"]["name"].as_str().unwrap_or("");
                let args = call["function"]["arguments"].as_object();
                
                let tool_result = self.execute_tool(db_pool, func_name, args).await;
                
                messages.push(serde_json::json!({
                    "role": "tool",
                    "content": tool_result
                }));
            }

            // Call again with tool results
            let follow_req = ChatRequest {
                model: self.model.clone(),
                messages,
                tools: vec![],
                stream: false,
            };
            
            res = self.client.post(&format!("{}/api/chat", self.ollama_url))
                .json(&follow_req)
                .send()
                .await?
                .json::<ChatResponse>()
                .await?;
        }

        Ok(res.message.content)
    }

    fn get_tools(&self) -> Vec<Value> {
        vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "create_company",
                    "description": "Create a new internal or external company.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "type": { "type": "string", "enum": ["INTERNAL", "EXTERNAL"] },
                            "description": { "type": "string" }
                        },
                        "required": ["name", "type"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "approve_request",
                    "description": "Approve a pending request (like a headcount increase).",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "request_id": { "type": "string" }
                        },
                        "required": ["request_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_pending_requests",
                    "description": "List all requests waiting for MainAgent approval.",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            })
            // Real version would have hire_ceo, reject_request, list_companies, view_ledger_summary, etc.
        ]
    }

    async fn execute_tool(&self, _db_pool: &sqlx::PgPool, name: &str, _args: Option<&serde_json::Map<String, Value>>) -> String {
        match name {
            "create_company" => {
                // e.g. execute DB insert for company. 
                // Return string result for the LLM context.
                "Company created successfully.".to_string()
            }
            "approve_request" => {
                // e.g. update approvals table
                "Request approved.".to_string()
            }
            "list_pending_requests" => {
                // e.g. query db where current_approver = MainAgent
                "No pending requests currently.".to_string()
            }
            _ => format!("Unknown tool: {}", name),
        }
    }
}
