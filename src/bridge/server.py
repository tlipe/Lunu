import sys
import os
import json
import logging
import asyncio
import secrets
from contextlib import asynccontextmanager
from fastapi import FastAPI, HTTPException, Depends, Request, Security
from fastapi.security import APIKeyHeader
from fastapi.middleware.cors import CORSMiddleware
from fastapi.middleware.trustedhost import TrustedHostMiddleware
import uvicorn

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

# --- Logging Setup ---
logging.basicConfig(
    level=getattr(logging, config["logging"]["level"]),
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    handlers=[
        logging.FileHandler(config["logging"]["file"]),
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
