//! Agent status and context persistence.
//!
//! Writes dynamic workspace files (STATUS.md, DIRECTIVES.md, RECENT_OUTPUTS.md,
//! TEAM_KNOWLEDGE.md) so agents have persistent, up-to-date context on every
//! interaction without relying on memory search.

use sqlx::PgPool;
use uuid::Uuid;

/// Build and write STATUS.md to the agent's workspace directory.
/// Non-fatal — returns Ok(()) even if individual queries fail.
pub async fn refresh_agent_status(
    db: &PgPool,
    data_dir: &std::path::Path,
    agent_id: Uuid,
) {
    let workspace_dir = data_dir.join(agent_id.to_string()).join("workspace");
    if !workspace_dir.exists() {
        return; // Agent workspace not yet created
    }

    match build_agent_status(db, agent_id).await {
        Ok(content) => {
            if let Err(e) = tokio::fs::write(workspace_dir.join("STATUS.md"), &content).await {
                tracing::warn!("Failed to write STATUS.md for {}: {}", agent_id, e);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to build STATUS.md for {}: {}", agent_id, e);
        }
    }
}

/// Build the STATUS.md content for an agent by querying the database.
/// Uses a single initial query for agent+parent info, then runs all remaining
/// queries concurrently via tokio::join! to minimize DB round-trips.
async fn build_agent_status(db: &PgPool, agent_id: Uuid) -> Result<String, sqlx::Error> {
    // Query 1: Agent info + parent name in one round-trip
    let agent: Option<(String, String, Option<Uuid>, Option<Uuid>, Option<String>)> = sqlx::query_as(
        "SELECT a.name, a.role, a.company_id, a.parent_agent_id, \
         (SELECT name FROM agents WHERE id = a.parent_agent_id) AS parent_name \
         FROM agents a WHERE a.id = $1"
    ).bind(agent_id).fetch_optional(db).await?;

    let (_agent_name, role, company_id, _parent_agent_id, parent_name) = match agent {
        Some(a) => a,
        None => return Ok(String::new()),
    };

    // Run all remaining queries concurrently based on role.
    // Each future resolves to its section string (empty if not applicable).
    let role_clone = role.clone();
    let role_clone2 = role.clone();
    let role_clone3 = role.clone();

    // Team roster / companies (depends on role)
    let team_fut = async {
        if role_clone == "WORKER" {
            return String::new();
        }
        if role_clone == "MAIN" {
            // For MAIN: company overview instead of team
            let companies: Vec<(String, String, Option<String>)> = sqlx::query_as(
                "SELECT c.name, c.type, c.description FROM companies c \
                 JOIN holdings h ON c.holding_id = h.id \
                 WHERE c.status = 'ACTIVE' ORDER BY c.name"
            ).fetch_all(db).await.unwrap_or_default();

            // Also fetch subordinates (CEOs)
            let team: Vec<(String, String, Option<String>)> = sqlx::query_as(
                "SELECT a.name, a.role, a.specialty FROM agents a \
                 WHERE a.parent_agent_id = $1 AND a.status = 'ACTIVE' \
                 ORDER BY a.role, a.name"
            ).bind(agent_id).fetch_all(db).await.unwrap_or_default();

            let mut result = String::new();
            if !team.is_empty() {
                result.push_str("## Your Team\n");
                for (name, r, spec) in &team {
                    let spec_str = spec.as_deref().unwrap_or("general");
                    let spec_short = truncate_str(spec_str, 60);
                    result.push_str(&format!("- {} ({}, {})\n", name, r, spec_short));
                }
            }
            if !companies.is_empty() {
                result.push_str("## Companies\n");
                for (name, ctype, desc) in &companies {
                    let desc_short = desc.as_deref().map(|d| {
                        format!(" — {}", truncate_str(d, 80))
                    }).unwrap_or_default();
                    result.push_str(&format!("- {} ({}){}\n", name, ctype, desc_short));
                }
            }
            result
        } else {
            // CEO/MANAGER: team roster
            let team: Vec<(String, String, Option<String>)> = sqlx::query_as(
                "SELECT a.name, a.role, a.specialty FROM agents a \
                 WHERE a.parent_agent_id = $1 AND a.status = 'ACTIVE' \
                 ORDER BY a.role, a.name"
            ).bind(agent_id).fetch_all(db).await.unwrap_or_default();

            if !team.is_empty() {
                let mut s = "## Your Team\n".to_string();
                for (name, r, spec) in &team {
                    let spec_str = spec.as_deref().unwrap_or("general");
                    let spec_short = truncate_str(spec_str, 60);
                    s.push_str(&format!("- {} ({}, {})\n", name, r, spec_short));
                }
                s
            } else {
                "## Your Team\nNo direct reports yet.\n".to_string()
            }
        }
    };

    // Recent activity
    let recent_fut = async {
        let recent: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT COALESCE(content->>'text', '') AS text, created_at FROM messages \
             WHERE sender_id = $1 AND sender_type = 'AGENT' \
             ORDER BY created_at DESC LIMIT 5"
        ).bind(agent_id).fetch_all(db).await.unwrap_or_default();

        if recent.is_empty() {
            return String::new();
        }
        let mut s = "## Recent Activity\n".to_string();
        let now = chrono::Utc::now();
        for (text, created_at) in &recent {
            let ago = format_duration_ago(now, *created_at);
            let preview = truncate_str(text, 100).replace('\n', " ");
            s.push_str(&format!("- [{}] {}\n", ago, preview));
        }
        s
    };

    // Pending requests + ledger (role-dependent, combined into one future)
    let extras_fut = async {
        let mut result = String::new();

        // Pending requests (MAIN/CEO only)
        if role_clone2 == "MAIN" || role_clone2 == "CEO" {
            let pending_count: Option<i64> = sqlx::query_scalar(
                "SELECT COUNT(*) FROM requests \
                 WHERE status = 'PENDING' AND current_approver_id = $1"
            ).bind(agent_id).fetch_optional(db).await.unwrap_or(None);

            if let Some(count) = pending_count {
                if count > 0 {
                    result.push_str(&format!("## Pending Requests\n{} request(s) awaiting your approval.\n", count));
                }
            }
        }

        // Ledger balance (CEO only)
        if role_clone2 == "CEO" {
            if let Some(cid) = company_id {
                let balance: Option<rust_decimal::Decimal> = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(CASE WHEN type = 'REVENUE' THEN amount \
                                            WHEN type = 'EXPENSE' THEN -amount \
                                            WHEN type = 'INTERNAL_TRANSFER' THEN amount \
                                            ELSE 0 END), 0) \
                     FROM ledger_entries WHERE company_id = $1"
                ).bind(cid).fetch_optional(db).await.unwrap_or(None);

                if let Some(bal) = balance {
                    result.push_str(&format!("## Company Ledger\nBalance: ${}\n", bal));
                }

                // Active engagements (as client or provider)
                let eng_count: Option<i64> = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM service_engagements \
                     WHERE (client_company_id = $1 OR provider_company_id = $1) AND status IN ('PENDING', 'ACTIVE')"
                ).bind(cid).fetch_optional(db).await.unwrap_or(None);
                if let Some(count) = eng_count {
                    if count > 0 {
                        result.push_str(&format!("## Active Engagements\n{} engagement(s) in progress.\n", count));
                    }
                }

                // Trading positions (non-zero holdings)
                let pos_count: Option<i64> = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT symbol) FROM trading_orders \
                     WHERE company_id = $1 AND status IN ('FILLED', 'PARTIAL')"
                ).bind(cid).fetch_optional(db).await.unwrap_or(None);
                if let Some(count) = pos_count {
                    if count > 0 {
                        result.push_str(&format!("## Trading\n{} symbol(s) traded.\n", count));
                    }
                }
            }
        }

        result
    };

    // Superior info — resolve from the already-fetched parent_name or look up MAIN
    let superior_fut = async {
        if role_clone3 == "MAIN" {
            return String::new();
        }
        if let Some(pname) = &parent_name {
            return format!("## Reports To\n{}\n", pname);
        }
        if role_clone3 == "CEO" {
            let main_name: Option<String> = sqlx::query_scalar(
                "SELECT name FROM agents WHERE role = 'MAIN' AND status = 'ACTIVE' LIMIT 1"
            ).fetch_optional(db).await.unwrap_or(None);
            if let Some(mname) = main_name {
                return format!("## Reports To\n{}\n", mname);
            }
        }
        String::new()
    };

    // Execute all in parallel
    let (team_section, recent_section, extras_section, superior_section) =
        tokio::join!(team_fut, recent_fut, extras_fut, superior_fut);

    // Assemble in display order
    let mut sections: Vec<String> = vec!["# Current Status\n".to_string()];
    if !team_section.is_empty() { sections.push(team_section); }
    if !superior_section.is_empty() { sections.push(superior_section); }
    if !recent_section.is_empty() { sections.push(recent_section); }
    if !extras_section.is_empty() { sections.push(extras_section); }

    Ok(sections.join("\n"))
}

/// Truncate a string to `max_chars` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let end = s.char_indices()
        .take_while(|(i, _)| *i < max_chars)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(max_chars);
    format!("{}...", &s[..end])
}

/// Format a duration as a human-readable "X ago" string.
fn format_duration_ago(now: chrono::DateTime<chrono::Utc>, then: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (now - then).num_seconds().max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hr ago", secs / 3600)
    } else {
        format!("{} days ago", secs / 86400)
    }
}

// ═══════════════════════════════════════════════════════════════
// Feature 3: Directive persistence
// ═══════════════════════════════════════════════════════════════

/// After a DM from a superior ends, append the key messages to the target's DIRECTIVES.md.
/// Only writes if the sender is the target's superior (parent_agent_id or MAIN for CEOs).
pub async fn append_directives(
    db: &PgPool,
    data_dir: &std::path::Path,
    sender_id: Uuid,
    target_id: Uuid,
    thread_id: Uuid,
) {
    // Check if sender is target's superior
    let target_info: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT role, parent_agent_id FROM agents WHERE id = $1"
    ).bind(target_id).fetch_optional(db).await.ok().flatten();

    let (target_role, parent_id) = match target_info {
        Some(info) => info,
        None => return,
    };

    let is_superior = match parent_id {
        Some(pid) => pid == sender_id,
        None => {
            // CEOs have no parent — check if sender is MAIN
            if target_role == "CEO" {
                let sender_role: Option<String> = sqlx::query_scalar(
                    "SELECT role FROM agents WHERE id = $1"
                ).bind(sender_id).fetch_optional(db).await.ok().flatten();
                sender_role.as_deref() == Some("MAIN")
            } else {
                false
            }
        }
    };

    if !is_superior {
        return; // Only persist directives from superiors
    }

    let workspace_dir = data_dir.join(target_id.to_string()).join("workspace");
    if !workspace_dir.exists() {
        return;
    }

    // Fetch sender's messages from the thread (the directives)
    let sender_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(sender_id).fetch_optional(db).await.ok().flatten()
        .unwrap_or_else(|| "Superior".into());

    let messages: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT COALESCE(content->>'text', ''), created_at FROM messages \
         WHERE thread_id = $1 AND sender_id = $2 AND sender_type = 'AGENT' \
         ORDER BY created_at ASC LIMIT 10"
    ).bind(thread_id).bind(sender_id)
    .fetch_all(db).await.unwrap_or_default();

    if messages.is_empty() {
        return;
    }

    // Build the directive block
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let mut block = format!("\n## From {} ({})\n", sender_name, timestamp);
    for (text, _) in &messages {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            block.push_str(trimmed);
            block.push('\n');
        }
    }

    // Read existing directives (or create new)
    let directives_path = workspace_dir.join("DIRECTIVES.md");
    let existing = tokio::fs::read_to_string(&directives_path).await.unwrap_or_default();

    let content = if existing.is_empty() {
        format!("# Directives Received\n{}", block)
    } else {
        // Cap at last 10 directive blocks
        let blocks: Vec<&str> = existing.split("\n## ").collect();
        let header = blocks.first().copied().unwrap_or("# Directives Received\n");
        let mut kept: Vec<String> = blocks.iter().skip(1).map(|b| format!("\n## {}", b)).collect();
        kept.push(block);
        // Keep only last 10
        if kept.len() > 10 {
            kept = kept.split_off(kept.len() - 10);
        }
        format!("{}{}", header, kept.join(""))
    };

    if let Err(e) = tokio::fs::write(&directives_path, &content).await {
        tracing::warn!("Failed to write DIRECTIVES.md for {}: {}", target_id, e);
    }
}

// ═══════════════════════════════════════════════════════════════
// Feature 2: Tool output persistence
// ═══════════════════════════════════════════════════════════════

/// Append a timestamped output entry to the agent's RECENT_OUTPUTS.md.
/// Capped at 50 entries (oldest removed on overflow).
pub async fn append_agent_output(
    data_dir: &std::path::Path,
    agent_id: Uuid,
    action: &str,
    result: &str,
) {
    let workspace_dir = data_dir.join(agent_id.to_string()).join("workspace");
    if !workspace_dir.exists() {
        return;
    }

    let outputs_path = workspace_dir.join("RECENT_OUTPUTS.md");
    let timestamp = chrono::Utc::now().format("%H:%M UTC").to_string();

    // Truncate long results
    let result_short = if result.len() > 300 {
        let end = result.char_indices().take_while(|(i, _)| *i < 300)
            .last().map(|(i, c)| i + c.len_utf8()).unwrap_or(300);
        format!("{}...", &result[..end])
    } else {
        result.to_string()
    };

    let entry = format!("- [{}] **{}**: {}\n", timestamp, action, result_short.replace('\n', " "));

    // Read existing entries
    let existing = tokio::fs::read_to_string(&outputs_path).await.unwrap_or_default();
    let mut lines: Vec<&str> = existing.lines()
        .filter(|l| l.starts_with("- ["))
        .collect();

    // Cap at 49 existing + 1 new = 50
    if lines.len() >= 50 {
        lines = lines.split_off(lines.len() - 49);
    }

    let mut content = "# Recent Outputs\n\n".to_string();
    for line in &lines {
        content.push_str(line);
        content.push('\n');
    }
    content.push_str(&entry);

    if let Err(e) = tokio::fs::write(&outputs_path, &content).await {
        tracing::warn!("Failed to write RECENT_OUTPUTS.md for {}: {}", agent_id, e);
    }
}

// ═══════════════════════════════════════════════════════════════
// Feature 4: Team knowledge base
// ═══════════════════════════════════════════════════════════════

/// Write TEAM_KNOWLEDGE.md to the agent's workspace from the team_knowledge table.
pub async fn refresh_team_knowledge(
    db: &PgPool,
    data_dir: &std::path::Path,
    agent_id: Uuid,
) {
    let workspace_dir = data_dir.join(agent_id.to_string()).join("workspace");
    if !workspace_dir.exists() {
        return;
    }

    // Determine scope: same company for CEO, same parent for workers/managers
    let agent_info: Option<(String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT role, company_id, parent_agent_id FROM agents WHERE id = $1"
    ).bind(agent_id).fetch_optional(db).await.ok().flatten();

    let (role, company_id, parent_agent_id) = match agent_info {
        Some(info) => info,
        None => return,
    };

    // For workers: show knowledge from siblings (same parent)
    // For managers: show knowledge from their workers
    // For CEOs: show knowledge from entire company
    // For MAIN: show knowledge from all companies
    let entries: Vec<(String, String, String, chrono::DateTime<chrono::Utc>)> = match role.as_str() {
        "WORKER" | "MANAGER" => {
            // Scope to same parent (team)
            let scope_id = if role == "WORKER" {
                parent_agent_id.unwrap_or(agent_id)
            } else {
                agent_id // Manager sees their own team's knowledge
            };
            sqlx::query_as(
                "SELECT a.name, tk.topic, tk.content, tk.created_at \
                 FROM team_knowledge tk \
                 JOIN agents a ON tk.agent_id = a.id \
                 WHERE a.parent_agent_id = $1 \
                 ORDER BY tk.created_at DESC LIMIT 20"
            ).bind(scope_id).fetch_all(db).await.unwrap_or_default()
        }
        "CEO" => {
            if let Some(cid) = company_id {
                sqlx::query_as(
                    "SELECT a.name, tk.topic, tk.content, tk.created_at \
                     FROM team_knowledge tk \
                     JOIN agents a ON tk.agent_id = a.id \
                     WHERE tk.company_id = $1 \
                     ORDER BY tk.created_at DESC LIMIT 30"
                ).bind(cid).fetch_all(db).await.unwrap_or_default()
            } else {
                vec![]
            }
        }
        "MAIN" => {
            sqlx::query_as(
                "SELECT a.name, tk.topic, tk.content, tk.created_at \
                 FROM team_knowledge tk \
                 JOIN agents a ON tk.agent_id = a.id \
                 ORDER BY tk.created_at DESC LIMIT 30"
            ).fetch_all(db).await.unwrap_or_default()
        }
        _ => vec![],
    };

    if entries.is_empty() {
        // Remove stale file if no entries
        let _ = tokio::fs::remove_file(workspace_dir.join("TEAM_KNOWLEDGE.md")).await;
        return;
    }

    let mut content = "# Team Knowledge Base\n\n".to_string();
    for (author, topic, body, created_at) in &entries {
        let date = created_at.format("%Y-%m-%d").to_string();
        // Truncate long bodies
        let body_short = if body.len() > 500 {
            let end = body.char_indices().take_while(|(i, _)| *i < 500)
                .last().map(|(i, c)| i + c.len_utf8()).unwrap_or(500);
            format!("{}...", &body[..end])
        } else {
            body.clone()
        };
        content.push_str(&format!("## {} (by {}, {})\n{}\n\n", topic, author, date, body_short));
    }

    if let Err(e) = tokio::fs::write(workspace_dir.join("TEAM_KNOWLEDGE.md"), &content).await {
        tracing::warn!("Failed to write TEAM_KNOWLEDGE.md for {}: {}", agent_id, e);
    }
}
