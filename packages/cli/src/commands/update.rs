use std::io::Write;
use anyhow::{bail, Result};
use colored::*;
use serde_json::json;
use crate::api;

pub async fn run(channel: Option<String>) -> Result<()> {
    // Validate and switch channel if requested
    if let Some(ref ch) = channel {
        match ch.as_str() {
            "stable" | "beta" | "dev" => {
                println!("Switching update channel to {}...", ch.cyan().bold());
                api::put("/system/settings", json!({"update_channel": ch})).await?;
                println!("  Channel set to {}", ch.green());
            }
            _ => bail!("Invalid channel '{}'. Must be one of: stable, beta, dev", ch),
        }
    }

    // Check for updates
    println!("Checking for updates...");
    let info = api::get("/system/update-check").await?;

    let current = info["current_version"].as_str().unwrap_or("unknown");
    let latest = info["latest_version"].as_str().unwrap_or("unknown");
    let available = info["update_available"].as_bool().unwrap_or(false);
    let ch = info["channel"].as_str().unwrap_or("unknown");

    println!("  Current: {} (channel: {})", current.cyan(), ch);
    println!("  Latest:  {}", latest.cyan());

    if !available && channel.is_none() {
        println!("\n{}", "Already up to date.".green().bold());
        return Ok(());
    }

    if !available && channel.is_some() {
        println!("\nChannel switched. No new update available on this channel.");
        return Ok(());
    }

    // Confirm
    println!();
    println!("  {} {} -> {}", "Update:".bold(), current, latest.green().bold());
    print!("  Proceed? [y/N] ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("  Cancelled.");
        return Ok(());
    }

    // Trigger update
    println!("\n{}", "Starting update...".yellow().bold());
    api::post("/system/update", None).await?;
    println!("  Update triggered. Containers are rebuilding...");
    println!("  Waiting for services to come back up...\n");

    // Poll health
    let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut i = 0;

    // Wait a bit for containers to start shutting down
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    for attempt in 0..120 {
        let frame = spinner[i % spinner.len()];
        i += 1;
        print!("\r  {} Waiting for API... ({}s)", frame, (attempt * 2) + 5);
        use std::io::Write;
        std::io::stdout().flush()?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        if api::health_ok().await {
            println!("\r  {} API is back up!              ", "✓".green().bold());
            break;
        }

        if attempt == 119 {
            println!("\r  {} Server not responding after 4 minutes.", "✗".red().bold());
            println!("  Check logs: multiclaw logs multiclawd");
            return Ok(());
        }
    }

    // Show new version
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    match api::get("/system/update-check").await {
        Ok(new_info) => {
            let new_ver = new_info["current_version"].as_str().unwrap_or("unknown");
            println!("\n{} Now running: {}", "Update complete!".green().bold(), new_ver.cyan().bold());
        }
        Err(_) => {
            println!("\n{}", "Update complete!".green().bold());
        }
    }

    Ok(())
}
