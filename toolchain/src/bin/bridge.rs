use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path as StdPath, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::process::Command;
use rand::RngCore;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone, Deserialize)]
struct ServerConfig {
    host: String,
    http_port: u16,
    ssl_enabled: bool,
    ssl_cert_path: String,
    ssl_key_path: String,
}

#[derive(Clone, Deserialize)]
struct SecurityConfig {
    auth_enabled: bool,
    allowed_hosts: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct LoggingConfig {
    level: String,
    file: String,
}

#[derive(Clone, Deserialize)]
struct Config {
    server: ServerConfig,
    security: SecurityConfig,
    logging: LoggingConfig,
    modules_dir: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Secrets {
    api_key: String,
}

#[derive(Clone)]
struct AppState {
    config: Config,
    secrets: Secrets,
    base_dir: PathBuf,
    modules_dir: PathBuf,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    args: Vec<Value>,
}

#[derive(Deserialize)]
struct BridgeConfig {
    commands: HashMap<String, CommandSpec>,
}

#[derive(Deserialize)]
struct CommandSpec {
    cmd: Vec<String>,
    cwd: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_dir = std::env::current_dir()?;
    let config = load_config(&base_dir)?;
    let secrets = load_or_create_secrets(&base_dir)?;
    let modules_dir = resolve_modules_dir(&base_dir, &config);

    let log_path = resolve_log_path(&base_dir, &config.logging.file);
    let log_dir = log_path.parent().unwrap_or_else(|| StdPath::new("."));
    std::fs::create_dir_all(log_dir)?;

    let file_appender = tracing_appender::rolling::never(log_dir, log_path.file_name().unwrap_or_default());
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let filter = tracing_subscriber::EnvFilter::new(config.logging.level.clone());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(tracing_subscriber::fmt::layer().with_writer(file_writer))
        .init();

    let host = config.server.host.clone();
    let port = config.server.http_port;
    let state = Arc::new(AppState {
        config,
        secrets,
        base_dir,
        modules_dir,
    });

    if state.config.server.ssl_enabled {
        return Err(anyhow::anyhow!("SSL is enabled in config but not supported by lunu-bridge."));
    }

    let protected = Router::new()
        .route("/api/v1/system/shutdown", post(shutdown))
        .route("/api/v1/system/info", get(system_info).post(system_info))
        .route("/api/v1/:module_name/:func_name", post(module_bridge))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(middleware::from_fn_with_state(state.clone(), host_middleware))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    info!("Lunu Bridge listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn load_config(base_dir: &PathBuf) -> anyhow::Result<Config> {
    let config_path = base_dir.join("config").join("settings.json");
    let content = std::fs::read_to_string(&config_path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}

fn resolve_log_path(base_dir: &PathBuf, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(path)
    }
}

fn resolve_modules_dir(base_dir: &PathBuf, config: &Config) -> PathBuf {
    let modules_dir = config.modules_dir.clone().unwrap_or_else(|| "modules".to_string());
    let candidate = PathBuf::from(modules_dir);
    if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    }
}

fn load_or_create_secrets(base_dir: &PathBuf) -> anyhow::Result<Secrets> {
    let secrets_path = base_dir.join("config").join(".secrets.json");
    if secrets_path.exists() {
        let content = std::fs::read_to_string(&secrets_path)?;
        let secrets: Secrets = serde_json::from_str(&content)?;
        return Ok(secrets);
    }

    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let api_key = hex::encode(bytes);
    let secrets = Secrets { api_key };
    let content = serde_json::to_string(&secrets)?;
    std::fs::write(&secrets_path, content)?;
    Ok(secrets)
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "system": "Lunu" }))
}

async fn system_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cwd = std::env::current_dir().ok().and_then(|p| p.to_str().map(|s| s.to_string()));
    let exe = std::env::current_exe().ok().and_then(|p| p.to_str().map(|s| s.to_string()));
    Json(json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cwd": cwd,
        "exe": exe,
        "modules_dir": state.modules_dir.to_string_lossy(),
    }))
}

async fn shutdown() -> impl IntoResponse {
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        std::process::exit(0);
    });
    Json(json!({ "result": "shutting down" }))
}

async fn module_bridge(
    Path((module_name, func_name)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Payload>,
) -> Result<impl IntoResponse, AppError> {
    if module_name == "system" {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Function not found"));
    }

    let module_dir = state.modules_dir.join(&module_name);
    if !module_dir.is_dir() {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Module not found"));
    }

    let cfg_path = module_dir.join("bridge.json");
    if !cfg_path.is_file() {
        return Err(AppError::new(StatusCode::NOT_FOUND, "Bridge config not found"));
    }

    let cfg_content = std::fs::read_to_string(&cfg_path)
        .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read bridge config"))?;
    let cfg: BridgeConfig = serde_json::from_str(&cfg_content)
        .map_err(|_| AppError::new(StatusCode::BAD_REQUEST, "Invalid bridge config"))?;

    let spec = cfg.commands.get(&func_name)
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Function not found"))?;

    if spec.cmd.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Invalid bridge command"));
    }

    let exec_path = resolve_exec_path(&module_dir, &spec.cmd[0]);
    let cwd = spec.cwd.as_ref().map(|p| resolve_cwd_path(&module_dir, p));
    let args = payload.args.iter().map(value_to_string).collect::<Vec<_>>();

    let mut cmd = Command::new(exec_path);
    if spec.cmd.len() > 1 {
        cmd.args(&spec.cmd[1..]);
    }
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().await.map_err(|e| {
        error!("Execution failed: {}", e);
        AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Execution failed")
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() { "Execution failed" } else { &stderr };
        return Err(AppError::new(StatusCode::INTERNAL_SERVER_ERROR, msg));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(Json(json!({ "result": stdout })))
}

fn resolve_exec_path(module_dir: &PathBuf, exec_path: &str) -> PathBuf {
    let candidate = PathBuf::from(exec_path);
    if candidate.is_absolute() {
        candidate
    } else {
        let local = module_dir.join(exec_path);
        if local.exists() {
            local
        } else {
            candidate
        }
    }
}

fn resolve_cwd_path(module_dir: &PathBuf, cwd: &str) -> PathBuf {
    let candidate = PathBuf::from(cwd);
    if candidate.is_absolute() {
        candidate
    } else {
        module_dir.join(cwd)
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "detail": self.message }));
        (self.status, body).into_response()
    }
}

async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> Result<Response, AppError> {
    if !state.config.security.auth_enabled {
        return Ok(next.run(request).await);
    }

    let api_key = headers
        .get("X-LUNU-KEY")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if api_key != state.secrets.api_key {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Invalid API Key"));
    }

    Ok(next.run(request).await)
}

async fn host_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> Result<Response, AppError> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if !host.is_empty() {
        let host_only = host.split(':').next().unwrap_or(host);
        if !state.config.security.allowed_hosts.iter().any(|h| h == host_only) {
            return Err(AppError::new(StatusCode::BAD_REQUEST, "Host not allowed"));
        }
    }

    Ok(next.run(request).await)
}
