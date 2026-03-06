use anyhow::Result;
use colored::*;
use crate::api;

pub async fn run() -> Result<()> {
    println!("{}", "MultiClaw System Info".bold());
    println!("{}", "─".repeat(40));

    // Static info
    println!("  API URL:       {}", "http://127.0.0.1:8080/v1".cyan());
    println!("  Dashboard URL: {}", "http://127.0.0.1:3000".cyan());
    println!("  Token file:    {}", "/var/lib/multiclaw/admin.token");
    println!("  Repo path:     {}", "/opt/multiclaw");
    println!("  Compose file:  {}", "/opt/multiclaw/infra/docker/docker-compose.yml");

    // Token preview
    match api::load_token() {
        Ok(token) => {
            let preview = if token.len() > 8 {
                format!("{}...", &token[..8])
            } else {
                token.clone()
            };
            println!("  Admin token:   {}", preview.dimmed());
        }
        Err(_) => println!("  Admin token:   {}", "not found".red()),
    }

    println!();

    // Dynamic info from API
    match api::get("/system/settings").await {
        Ok(settings) => {
            let channel = settings["update_channel"].as_str().unwrap_or("stable");
            let commit = settings["deployed_commit"].as_str().unwrap_or("unknown");
            println!("  Update channel:  {}", channel.cyan());
            println!("  Deployed commit: {}", commit);
        }
        Err(_) => {
            println!("  {} (API unreachable)", "Could not fetch settings".yellow());
            // Fallback to git
            let output = std::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--short", "HEAD"])
                .output();
            if let Ok(o) = output {
                if o.status.success() {
                    let sha = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    println!("  Git HEAD:        {}", sha);
                }
            }
        }
    }

    Ok(())
}
