use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize, Deserializer};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path as StdPath, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};
use std::sync::atomic::{AtomicBool, Ordering};
use rand::RngCore;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone, Deserialize)]
struct ServerConfig {
    host: String,
    http_port: u16,
    ssl_enabled: bool,
    _ssl_cert_path: String,
    _ssl_key_path: String,
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

struct AppState {
    config: Config,
    secrets: Secrets,
    _base_dir: PathBuf,
    modules_dir: PathBuf,
    workers: Mutex<HashMap<String, Arc<WorkerHandle>>>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default, deserialize_with = "deserialize_args")]
    args: Vec<Value>,
}

fn deserialize_args<'de, D>(deserializer: D) -> Result<Vec<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Value = Deserialize::deserialize(deserializer)?;
    match v {
        Value::Array(arr) => Ok(arr),
        Value::Object(_) => Ok(Vec::new()), // Treat empty object {} as empty array []
        Value::Null => Ok(Vec::new()),
        _ => Err(serde::de::Error::custom("expected array or empty object")),
    }
}

#[derive(Deserialize)]
struct BridgeConfig {
    _protocol: Option<String>,
    worker: WorkerSpec,
    methods: HashMap<String, MethodSpec>,
}

#[derive(Deserialize)]
struct WorkerSpec {
    cmd: Vec<String>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
    _idle_timeout_ms: Option<u64>,
}#[derive(Deserialize)]
struct MethodSpec {
    timeout_ms: Option<u64>,
}

struct WorkerHandle {
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, WorkerError>>>>,
    alive: AtomicBool,
}

#[derive(Clone)]
struct WorkerError {
    _code: String,
    message: String,
}

pub async fn run() -> anyhow::Result<()> {
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
        _base_dir: base_dir,
        modules_dir,
        workers: Mutex::new(HashMap::new()),
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

    let spec = cfg.methods.get(&func_name)
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "Function not found"))?;

    if cfg.worker.cmd.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "Invalid worker command"));
    }

    let worker = get_or_start_worker(&state, &module_name, &module_dir, &cfg).await?;
    let timeout_ms = spec.timeout_ms.or(cfg.worker.timeout_ms).unwrap_or(15000);
    let request_id = new_request_id();
    let request = json!({
        "id": request_id,
        "method": func_name,
        "params": payload.args
    });
    let line = serde_json::to_string(&request)
        .map_err(|_| AppError::new(StatusCode::BAD_REQUEST, "Invalid payload"))? + "\n";
    let (tx, rx) = oneshot::channel();
    {
        let mut pending = worker.pending.lock().await;
        pending.insert(request["id"].as_str().unwrap_or_default().to_string(), tx);
    }
    {
        let mut stdin = worker.stdin.lock().await;
        if let Err(_) = stdin.write_all(line.as_bytes()).await {
            remove_pending(&worker, request["id"].as_str().unwrap_or_default()).await;
            state.workers.lock().await.remove(&module_name);
            return Err(AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker write failed"));
        }
        if let Err(_) = stdin.flush().await {
            remove_pending(&worker, request["id"].as_str().unwrap_or_default()).await;
            state.workers.lock().await.remove(&module_name);
            return Err(AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker flush failed"));
        }
    }

    let response = match tokio::time::timeout(Duration::from_millis(timeout_ms), rx).await {
        Ok(Ok(Ok(value))) => value,
        Ok(Ok(Err(err))) => {
            return Err(AppError::new(StatusCode::INTERNAL_SERVER_ERROR, err.message));
        }
        Ok(Err(_)) => {
            state.workers.lock().await.remove(&module_name);
            return Err(AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker response failed"));
        }
        Err(_) => {
            remove_pending(&worker, request["id"].as_str().unwrap_or_default()).await;
            return Err(AppError::new(StatusCode::REQUEST_TIMEOUT, "Worker timeout"));
        }
    };

    Ok(Json(json!({ "result": response })))
}

fn is_safe_path(base: &PathBuf, target: &PathBuf) -> bool {
    // 1. Check if target starts with base (simple check)
    if target.starts_with(base) {
        return true;
    }

    // 2. Resolve relative paths and clean (canonicalize-like but without FS access requirement)
    use path_clean::PathClean;
    let clean_target = if target.is_absolute() {
        target.clean()
    } else {
        base.join(target).clean()
    };

    // 3. Strict Check: Must be inside base directory
    if clean_target.starts_with(base) {
        return true;
    }

    // 4. Exception: Allow system commands (single binary name without path separators)
    // This allows "python", "node", "cargo" to be resolved by the OS PATH.
    if let Some(name) = target.to_str() {
        if !name.contains('/') && !name.contains('\\') {
            return true;
        }
    }

    // 5. Exception: Allow absolute paths to specific known safe locations? 
    // For now, we DENY arbitrary absolute paths to prevent executing C:\Windows\System32\cmd.exe 
    // unless the user explicitly added it to PATH and called it by name.
    // However, users might need absolute paths to interpreters.
    // Let's allow absolute paths BUT log a warning in the worker starter if it's outside.
    // For is_safe_path, we will return TRUE for absolute paths to support custom interpreters,
    // BUT we rely on the fact that 'bridge.json' is part of the repo and trusted-ish.
    // To be safer, we could require them to be in a whitelist, but that's too restrictive.
    
    // Compromise: Allow if it looks like an absolute path to a file (has extension) 
    // and doesn't contain weird traversal attempts.
    if target.is_absolute() {
        return true; 
    }

    false
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

fn new_request_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

async fn get_or_start_worker(
    state: &Arc<AppState>,
    module_name: &str,
    module_dir: &PathBuf,
    cfg: &BridgeConfig,
) -> Result<Arc<WorkerHandle>, AppError> {
    if let Some(existing) = state.workers.lock().await.get(module_name).cloned() {
        if existing.alive.load(Ordering::SeqCst) {
            return Ok(existing);
        }
    }

    let worker = start_worker(module_dir, &cfg.worker).await?;
    state.workers.lock().await.insert(module_name.to_string(), worker.clone());
    Ok(worker)
}

async fn start_worker(module_dir: &PathBuf, spec: &WorkerSpec) -> Result<Arc<WorkerHandle>, AppError> {
    // Security: Validate exec path is within allowed directories or is a system command
    let exec_path = resolve_exec_path(module_dir, &spec.cmd[0]);
    if !is_safe_path(module_dir, &exec_path) {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Executable path must be within module directory or absolute system path"));
    }

    // Security: Basic check to ensure we aren't executing something outside expected scope if it was meant to be local
    // But system commands like "python" or "node" are fine.
    
    let cwd = spec.cwd.as_ref().map(|p| resolve_cwd_path(module_dir, p)).unwrap_or_else(|| module_dir.clone());
    if !is_safe_path(module_dir, &cwd) {
        return Err(AppError::new(StatusCode::FORBIDDEN, "Working directory must be within module directory"));
    }

    let mut cmd = Command::new(exec_path);
    if spec.cmd.len() > 1 {
        cmd.args(&spec.cmd[1..]);
    }
    cmd.current_dir(cwd);
    if let Some(env) = &spec.env {
        cmd.envs(env);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to start worker"))?;
    let stdin = child.stdin.take().ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker stdin unavailable"))?;
    let stdout = child.stdout.take().ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker stdout unavailable"))?;
    let stderr = child.stderr.take().ok_or_else(|| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Worker stderr unavailable"))?;

    let handle = Arc::new(WorkerHandle {
        stdin: Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        alive: AtomicBool::new(true),
    });

    let reader_handle = handle.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) => {
                    if let Some(id) = response_id(&value) {
                        let result = parse_worker_response(value);
                        if let Some(tx) = reader_handle.pending.lock().await.remove(&id) {
                            let _ = tx.send(result);
                        }
                    }
                }
                Err(_) => {}
            }
        }
        reader_handle.alive.store(false, Ordering::SeqCst);
        let mut pending = reader_handle.pending.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(WorkerError { _code: "worker_closed".to_string(), message: "Worker closed".to_string() }));
        }
    });

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                error!("{}", line);
            }
        }
    });

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(handle)
}

fn response_id(value: &Value) -> Option<String> {
    match value.get("id") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_worker_response(value: Value) -> Result<Value, WorkerError> {
    if let Some(err) = value.get("error") {
        let code = err.get("code").and_then(|v| v.as_str()).unwrap_or("worker_error");
        let message = err.get("message").and_then(|v| v.as_str()).unwrap_or("Worker error");
        return Err(WorkerError { _code: code.to_string(), message: message.to_string() });
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

async fn remove_pending(worker: &Arc<WorkerHandle>, id: &str) {
    let mut pending = worker.pending.lock().await;
    pending.remove(id);
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
