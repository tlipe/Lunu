import sys
import json


def normalize_params(params):
    if len(params) == 1 and isinstance(params[0], list):
        return params[0]
    return params


def handle(method, params):
    params = normalize_params(params)
    if method == "hello":
        return {"result": "Hello from Python"}
    if method == "echo":
        if len(params) > 0:
            return {"result": params[0]}
        return {"result": None}
    return {"error": {"code": "method_not_found", "message": "Method not found"}}


def main():
    while True:
        line = sys.stdin.readline()
        if line == "":
            break
        line = line.strip()
        if line == "":
            continue
        try:
            payload = json.loads(line)
        except Exception:
            continue
        request_id = payload.get("id")
        method = payload.get("method")
        params = payload.get("params", [])
        if request_id is None or method is None:
            continue
        response = handle(method, params)
        response["id"] = request_id
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
