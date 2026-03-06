use anyhow::Result;
use colored::*;
use crate::api;

pub async fn run() -> Result<()> {
    println!("{}", "MultiClaw Status".bold());
    println!("{}", "─".repeat(40));

    // API Health
    let api_up = api::health_ok().await;
    if api_up {
        println!("  API (port 8080):       {}", "UP".green().bold());
    } else {
        println!("  API (port 8080):       {}", "DOWN".red().bold());
        println!("\n  Control-plane API is not reachable.");
        println!("  Try: docker compose -f /opt/multiclaw/infra/docker/docker-compose.yml logs multiclawd");
        return Ok(());
    }

    // Dashboard
    let dash_ok = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
        .get("http://127.0.0.1:3000")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    if dash_ok {
        println!("  Dashboard (port 3000): {}", "UP".green().bold());
    } else {
        println!("  Dashboard (port 3000): {}", "DOWN".red().bold());
    }

    println!();

    // Containers
    match api::get("/system/containers").await {
        Ok(containers) => {
            if let Some(arr) = containers.as_array() {
                println!("{}", "Containers:".bold());
                for c in arr {
                    let name = c["Names"].as_str().unwrap_or("?");
                    let state = c["State"].as_str().unwrap_or("?");
                    let status = c["Status"].as_str().unwrap_or("?");
                    let state_colored = match state {
                        "running" => state.green().bold(),
                        "exited" => state.red().bold(),
                        _ => state.yellow().bold(),
                    };
                    println!("  {:<28} {}  ({})", name, state_colored, status.dimmed());
                }
            }
        }
        Err(e) => println!("  {}: {}", "Could not fetch containers".red(), e),
    }

    println!();

    // Update check
    match api::get("/system/update-check").await {
        Ok(info) => {
            let current = info["current_version"].as_str().unwrap_or("unknown");
            let available = info["update_available"].as_bool().unwrap_or(false);
            let channel = info["channel"].as_str().unwrap_or("unknown");
            println!("  Version: {}  (channel: {})", current.cyan(), channel);
            if available {
                let latest = info["latest_version"].as_str().unwrap_or("?");
                println!("  {} {} available (run: multiclaw update)", "Update".green().bold(), latest);
            } else {
                println!("  {}", "Up to date".green());
            }
        }
        Err(_) => println!("  Version: {}", "could not check".yellow()),
    }

    Ok(())
}
