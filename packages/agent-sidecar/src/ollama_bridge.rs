use crate::config::Config;
use anyhow::Result;
use axum::{
    body::Body,
    extract::State,
    http::Request,
    response::Response,
    routing::any,
    Router,
};
use std::sync::Arc;

struct BridgeState {
    client: reqwest::Client,
    host_proxy_url: String,
    ollama_token: String,
}

pub async fn start(cfg: Config) -> Result<()> {
    let state = Arc::new(BridgeState {
        client: reqwest::Client::new(),
        host_proxy_url: cfg.host_ollama_proxy_url,
        ollama_token: cfg.ollama_token,
    });

    let app = Router::new()
        .route("/*path", any(proxy_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:11435").await?;
    tracing::info!("Ollama Bridge listening on 127.0.0.1:11435");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn proxy_handler(
    State(state): State<Arc<BridgeState>>,
    mut req: Request<Body>,
) -> Result<Response<Body>, axum::http::StatusCode> {
    let path = req.uri().path();
    let query = req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
    let target_url = format!("{}{}{}", state.host_proxy_url, path, query);

    let (parts, body) = req.into_parts();
    // Convert axum body stream into reqwest body
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());

    let mut builder = state.client.request(parts.method, &target_url)
        .bearer_auth(&state.ollama_token)
        .body(reqwest_body);

    for (header_name, header_value) in parts.headers.iter() {
        // filter hop-by-hop if needed, but for MVP keep it simple
        builder = builder.header(header_name.as_str(), header_value);
    }

    let res = builder.send().await.map_err(|e| {
        tracing::error!("Proxy upstream error: {}", e);
        axum::http::StatusCode::BAD_GATEWAY
    })?;

    let mut response_builder = Response::builder()
        .status(res.status());

    for (header_name, header_value) in res.headers().iter() {
        response_builder = response_builder.header(header_name.clone(), header_value.clone());
    }

    let stream = res.bytes_stream();
    let body = Body::from_stream(stream);
    
    Ok(response_builder.body(body).unwrap())
}
