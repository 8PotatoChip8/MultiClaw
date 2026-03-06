use anyhow::Result;
use colored::*;
use crate::api;

pub async fn run() -> Result<()> {
    match api::get("/system/update-check").await {
        Ok(info) => {
            let current = info["current_version"].as_str().unwrap_or("unknown");
            let channel = info["channel"].as_str().unwrap_or("unknown");
            let semver = info["semver"].as_str().unwrap_or("0.1.0");
            let commit = info["deployed_commit"].as_str().unwrap_or("");

            println!("{} {}", "MultiClaw".bold(), current.cyan().bold());
            println!("  Semver:  {}", semver);
            println!("  Channel: {}", channel);
            if !commit.is_empty() {
                println!("  Commit:  {}", commit);
            }

            let available = info["update_available"].as_bool().unwrap_or(false);
            if available {
                let latest = info["latest_version"].as_str().unwrap_or("?");
                let msg = info["commit_message"].as_str().unwrap_or("");
                println!();
                println!("  {} {} -> {}", "Update available:".green().bold(), current, latest.green());
                if !msg.is_empty() {
                    println!("  Latest commit: {}", msg.dimmed());
                }
                println!("  Run: {}", "multiclaw update".yellow());
            }
        }
        Err(_) => {
            // Fallback: try git directly
            eprintln!("  API unreachable, trying git fallback...");
            let output = std::process::Command::new("git")
                .args(["-C", "/opt/multiclaw", "rev-parse", "--short", "HEAD"])
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let sha = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    println!("{} {}", "MultiClaw".bold(), sha.cyan());
                    println!("  (API is down — showing git SHA only)");
                }
                _ => {
                    println!("MultiClaw version: {}", "unknown (API down, git unavailable)".red());
                }
            }
        }
    }

    Ok(())
}
