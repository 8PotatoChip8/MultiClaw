use anyhow::{bail, Result};

const COMPOSE_FILE: &str = "/opt/multiclaw/infra/docker/docker-compose.yml";
const VALID_SERVICES: &[&str] = &["multiclawd", "ui", "postgres", "ollama-proxy"];

pub async fn run(service: Option<String>, tail: u32) -> Result<()> {
    let svc = service.unwrap_or_else(|| "multiclawd".to_string());

    if !VALID_SERVICES.contains(&svc.as_str()) {
        bail!(
            "Unknown service '{}'. Valid services: {}",
            svc,
            VALID_SERVICES.join(", ")
        );
    }

    let tail_str = tail.to_string();

    // Use std::process::Command with inherited stdio so logs stream to terminal
    let status = std::process::Command::new("docker")
        .args(["compose", "-f", COMPOSE_FILE, "logs", &svc, "--tail", &tail_str, "-f"])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("docker compose logs exited with status {}", status);
    }

    Ok(())
}
