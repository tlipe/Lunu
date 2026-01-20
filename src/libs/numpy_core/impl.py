import numpy as np

# Core NumPy Logic extracted and improved
# Designed to be framework-agnostic (used by both TCP and HTTP bridges)

def execute_numpy_function(func_name: str, args: list):
    """
    Executes a numpy function safely.
    """
    if not hasattr(np, func_name) and func_name not in CUSTOM_FUNCTIONS:
        raise ValueError(f"Function '{func_name}' not found or not allowed.")

    # Logic from original numpy_.py adapted here
    if func_name in CUSTOM_FUNCTIONS:
        return CUSTOM_FUNCTIONS[func_name](args)
    
    # Generic safety wrapper could go here, but for now we stick to the allowed list pattern
    # for security (avoiding arbitrary code execution via getattr if possible, 
    # but the original code had a specific map. We will reproduce that map for compatibility).
    
    if func_name in COMPAT_MAP:
        return COMPAT_MAP[func_name](args)
        
    return None

def _process_input(v):
    return np.array(v) if isinstance(v, list) else v

COMPAT_MAP = {
    "mean": lambda v: float(np.mean(_process_input(v))) if isinstance(v, list) else None,
    "median": lambda v: float(np.median(_process_input(v))) if isinstance(v, list) else None,
    "sum": lambda v: float(np.sum(_process_input(v))) if isinstance(v, list) else None,
    "std": lambda v: float(np.std(_process_input(v))) if isinstance(v, list) else None,
    "var": lambda v: float(np.var(_process_input(v))) if isinstance(v, list) else None,
    "min": lambda v: float(np.min(_process_input(v))) if isinstance(v, list) else None,
    "max": lambda v: float(np.max(_process_input(v))) if isinstance(v, list) else None,
    "sqrt": lambda v: (
        np.sqrt(_process_input(v)).tolist() if isinstance(v, list) else float(np.sqrt(v))
    ) if isinstance(v, (list, int, float)) else None,
    "log": lambda v: (
        np.log(_process_input(v)).tolist() if isinstance(v, list) else float(np.log(v))
    ) if isinstance(v, (list, int, float)) else None,
    "exp": lambda v: (
        np.exp(_process_input(v)).tolist() if isinstance(v, list) else float(np.exp(v))
    ) if isinstance(v, (list, int, float)) else None,
    "abs": lambda v: (
        np.abs(_process_input(v)).tolist() if isinstance(v, list) else float(np.abs(v))
    ) if isinstance(v, (list, int, float)) else None,
    "sin": lambda v: (
        np.sin(_process_input(v)).tolist() if isinstance(v, list) else float(np.sin(v))
    ) if isinstance(v, (list, int, float)) else None,
    "cos": lambda v: (
        np.cos(_process_input(v)).tolist() if isinstance(v, list) else float(np.cos(v))
    ) if isinstance(v, (list, int, float)) else None,
    "round": lambda v: (
        np.round(_process_input(v)).tolist() if isinstance(v, list) else float(np.round(v))
    ) if isinstance(v, (list, int, float)) else None,
}

CUSTOM_FUNCTIONS = {}
