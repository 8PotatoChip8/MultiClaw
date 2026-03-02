use crate::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};

// Target local Ollama process without auth
const OLLAMA_LOCAL_URL: &str = "http://127.0.0.1:11434";

pub async fn proxy_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let path = req.uri().path();
    let query = req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
    let target_url = format!("{}{}{}", OLLAMA_LOCAL_URL, path, query);

    let (parts, body) = req.into_parts();
    // Wrap to reqwest::Body
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());

    // Convert axum Method to reqwest Method
    let reqwest_method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes()).unwrap();

    let mut builder = state.client.request(reqwest_method, &target_url).body(reqwest_body);

    for (header_name, header_value) in parts.headers.iter() {
        if header_name != "host" && header_name != "authorization" {
            // Convert axum HeaderValue to reqwest HeaderValue
            let reqwest_header_value = reqwest::header::HeaderValue::from_bytes(header_value.as_bytes()).unwrap();
            builder = builder.header(header_name.as_str(), reqwest_header_value);
        }
    }

    let res = builder.send().await.map_err(|e| {
        tracing::error!("Proxy upstream error to local ollama: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    // Convert reqwest StatusCode to axum StatusCode
    let axum_status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut response_builder = Response::builder().status(axum_status);

    for (header_name, header_value) in res.headers().iter() {
        // Convert reqwest Header to axum Header
        let axum_header_name = axum::http::HeaderName::from_bytes(header_name.as_str().as_bytes()).unwrap();
        let axum_header_value = axum::http::HeaderValue::from_bytes(header_value.as_bytes()).unwrap();
        response_builder = response_builder.header(axum_header_name, axum_header_value);
    }

    let stream = res.bytes_stream();
    let axum_body = Body::from_stream(stream);
    
    Ok(response_builder.body(axum_body).unwrap())
}
