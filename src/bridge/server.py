import sys
import os
import json
import logging
import asyncio
import secrets
import subprocess
from contextlib import asynccontextmanager
from fastapi import FastAPI, HTTPException, Depends, Request, Security
from fastapi.security import APIKeyHeader
from fastapi.middleware.cors import CORSMiddleware
from fastapi.middleware.trustedhost import TrustedHostMiddleware
import uvicorn
from urllib.parse import urlparse

# Adjust path to find libs
sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), "../..")))

from src.libs.numpy_core.impl import execute_numpy_function

# --- Config Loading ---
CONFIG_PATH = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../config/settings.json"))
SECRETS_PATH = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../config/.secrets.json"))

def load_config():
    with open(CONFIG_PATH, "r") as f:
        return json.load(f)

config = load_config()

def resolve_modules_dir():
    base = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
    modules_dir = config.get("modules_dir", "modules")
    if os.path.isabs(modules_dir):
        return modules_dir
    return os.path.join(base, modules_dir)

def load_bridge_config(module_dir):
    path = os.path.join(module_dir, "bridge.json")
    if not os.path.isfile(path):
        return None
    with open(path, "r") as f:
        return json.load(f)

def resolve_command(module_dir, spec, args):
    cmd = spec.get("cmd")
    if not isinstance(cmd, list) or len(cmd) == 0:
        raise HTTPException(status_code=400, detail="Invalid bridge command")
    exec_path = cmd[0]
    if not os.path.isabs(exec_path):
        candidate = os.path.join(module_dir, exec_path)
        if os.path.exists(candidate):
            exec_path = candidate
    cwd = spec.get("cwd")
    if cwd and not os.path.isabs(cwd):
        cwd = os.path.join(module_dir, cwd)
    final_cmd = [exec_path] + cmd[1:] + [str(a) for a in args]
    return final_cmd, cwd

# Ensure logs dir exists before setting up logging
log_file = config["logging"]["file"]
# Resolve path relative to where script is run if not absolute
if not os.path.isabs(log_file):
    # If using ./logs/lunu.log, it will be relative to CWD
    log_file = os.path.abspath(log_file)

os.makedirs(os.path.dirname(log_file), exist_ok=True)

# --- Logging Setup ---
logging.basicConfig(
    level=getattr(logging, config["logging"]["level"]),
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    handlers=[
        logging.FileHandler(log_file),
        logging.StreamHandler()
    ]
)
logger = logging.getLogger("lunu.server")

# --- Security ---
API_KEY_NAME = "X-LUNU-KEY"
api_key_header = APIKeyHeader(name=API_KEY_NAME, auto_error=False)

def get_api_key():
    if not os.path.exists(SECRETS_PATH):
        # Generate on fly if missing (first run)
        key = secrets.token_urlsafe(32)
        with open(SECRETS_PATH, "w") as f:
            json.dump({"api_key": key}, f)
        return key
    
    with open(SECRETS_PATH, "r") as f:
        data = json.load(f)
        return data.get("api_key")

SERVER_API_KEY = get_api_key()

async def verify_api_key(api_key_header: str = Security(api_key_header)):
    if not config["security"]["auth_enabled"]:
        return True
    if api_key_header == SERVER_API_KEY:
        return True
    raise HTTPException(status_code=403, detail="Invalid API Key")

# --- App Lifecycle ---
@asynccontextmanager
async def lifespan(app: FastAPI):
    # Startup logic can be added here if needed
    yield
    # Shutdown logic if needed

app = FastAPI(title="Lunu Secure Bridge", lifespan=lifespan)

# --- Middleware ---
app.add_middleware(
    TrustedHostMiddleware, 
    allowed_hosts=config["security"]["allowed_hosts"]
)

@app.middleware("http")
async def log_requests(request: Request, call_next):
    raw_path = request.scope.get("path", "")
    if raw_path.startswith("http://") or raw_path.startswith("https://"):
        parsed = urlparse(raw_path)
        request.scope["path"] = parsed.path or "/"
        request.scope["raw_path"] = request.scope["path"].encode()
        if parsed.query:
            request.scope["query_string"] = parsed.query.encode()
    logger.info(f"HTTP Request: {request.method} {request.url} - Client: {request.client.host}")
    response = await call_next(request)
    return response

# --- Routes ---

@app.get("/health")
async def health():
    return {"status": "ok", "system": "Lunu"}

@app.post("/api/v1/numpy/{func_name}", dependencies=[Depends(verify_api_key)])
async def call_numpy(func_name: str, payload: dict):
    """
    Secure endpoint to call Numpy functions.
    Payload should be {"args": [val1, val2]}
    """
    args = payload.get("args", [])
    try:
        result = execute_numpy_function(func_name, args)
        return {"result": result}
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        logger.error(f"Execution error: {e}")
        raise HTTPException(status_code=500, detail="Internal Execution Error")

@app.post("/api/v1/system/shutdown", dependencies=[Depends(verify_api_key)])
async def shutdown_server():
    async def _shutdown():
        await asyncio.sleep(0.5)
        os._exit(0)
    asyncio.create_task(_shutdown())
    return {"result": "shutting down"}

@app.post("/api/v1/{module_name}/{func_name}", dependencies=[Depends(verify_api_key)])
async def module_bridge(module_name: str, func_name: str, payload: dict):
    if module_name in ["numpy", "system"]:
        raise HTTPException(status_code=404, detail="Function not found")

    modules_dir = resolve_modules_dir()
    module_dir = os.path.join(modules_dir, module_name)
    if not os.path.isdir(module_dir):
        raise HTTPException(status_code=404, detail="Module not found")

    cfg = load_bridge_config(module_dir)
    if not cfg:
        raise HTTPException(status_code=404, detail="Bridge config not found")

    commands = cfg.get("commands", {})
    spec = commands.get(func_name)
    if not spec:
        raise HTTPException(status_code=404, detail="Function not found")

    args = payload.get("args", [])
    cmd, cwd = resolve_command(module_dir, spec, args)
    try:
        result = subprocess.check_output(cmd, cwd=cwd, text=True)
        return {"result": result.strip()}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.api_route("/api/v1/system/info", methods=["GET", "POST"], dependencies=[Depends(verify_api_key)])
async def system_info(request: Request):
    """
    Returns system stats (CPU, Disk, OS). Supports GET and POST.
    """
    return {"result": get_system_info()}

if __name__ == "__main__":
    # Ensure logs dir exists
    os.makedirs(os.path.dirname(config["logging"]["file"]), exist_ok=True)
    
    ssl_config = {}
    if config["server"]["ssl_enabled"]:
        ssl_config = {
            "ssl_keyfile": config["server"]["ssl_key_path"],
            "ssl_certfile": config["server"]["ssl_cert_path"]
        }

    uvicorn.run(
        "server:app",
        host=config["server"]["host"],
        port=config["server"]["http_port"],
        workers=config["server"]["workers"],
        **ssl_config
    )
