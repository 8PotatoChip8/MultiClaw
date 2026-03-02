use anyhow::Result;

pub struct CloudInitArgs {
    pub hostname: String,
    pub host_ip: String,
    pub agent_id: String,
    pub agent_name: String,
    pub effective_model: String,
    pub agent_token: String,
    pub openclaw_gateway_token: String,
    pub ollama_token: String,
    pub tools_allow: String, // JSON array string
    pub tools_deny: String,  // JSON array string
    
    // Templates read from disk or embedded
    pub tmpl_user_data: String,
    pub tmpl_openclaw_json: String,
    pub tmpl_openclaw_svc: String,
    pub tmpl_agentd_svc: String,
}

pub fn render_cloud_init(args: &CloudInitArgs) -> Result<String> {
    let openclaw_json = args.tmpl_openclaw_json
        .replace("{{OPENCLAW_GATEWAY_TOKEN}}", &args.openclaw_gateway_token)
        .replace("{{EFFECTIVE_MODEL}}", &args.effective_model)
        .replace("{{AGENT_NAME}}", &args.agent_name)
        .replace("{{TOOLS_ALLOW}}", &args.tools_allow)
        .replace("{{TOOLS_DENY}}", &args.tools_deny);

    let openclaw_svc = args.tmpl_openclaw_svc
        .replace("{{OPENCLAW_GATEWAY_TOKEN}}", &args.openclaw_gateway_token);

    let agentd_svc = args.tmpl_agentd_svc
        .replace("{{AGENT_ID}}", &args.agent_id)
        .replace("{{AGENTD_TOKEN}}", &args.agent_token)
        .replace("{{OLLAMA_TOKEN}}", &args.ollama_token)
        .replace("{{HOST_IP}}", &args.host_ip);

    // Escape multi-line config files for embedding into the bash runcmd or write_files in cloud-init
    // The write_files block uses `|` so we just need to ensure indentation is correct.
    let indent = "      ";
    let openclaw_json_indented = openclaw_json.replace('\n', &format!("\n{}", indent));
    let openclaw_svc_indented = openclaw_svc.replace('\n', &format!("\n{}", indent));
    let agentd_svc_indented = agentd_svc.replace('\n', &format!("\n{}", indent));

    let final_yaml = args.tmpl_user_data
        .replace("{{HOSTNAME}}", &args.hostname)
        .replace("{{HOST_IP}}", &args.host_ip)
        .replace("{{OPENCLAW_JSON}}", &openclaw_json_indented)
        .replace("{{OPENCLAW_SERVICE}}", &openclaw_svc_indented)
        .replace("{{AGENTD_SERVICE}}", &agentd_svc_indented);

    Ok(final_yaml)
}
