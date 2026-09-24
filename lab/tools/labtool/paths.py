from __future__ import annotations
import hashlib, json, os, subprocess, platform
from pathlib import Path

LAB = Path(__file__).resolve().parents[2]          # .../shaderopt/lab
ROOT = LAB.parent
IGL = Path.home() / "sources" / "igl"
GLSLANG_BUILD = IGL / "third-party/deps/src/glslang/build"
GLSLANG = GLSLANG_BUILD / "StandAlone/glslang"
SPIRV_TOOLS = GLSLANG_BUILD / "External/spirv-tools/tools"
SPIRV_VAL = SPIRV_TOOLS / "spirv-val"
SPIRV_OPT = SPIRV_TOOLS / "spirv-opt"
SPIRV_DIS = SPIRV_TOOLS / "spirv-dis"
SPIRV_CROSS = IGL / "third-party/deps/src/SPIRV-Cross/build/spirv-cross"
RUNNER = LAB / "runner/build/shaderlab-runner"
SHADER_IR = LAB / "crates/shader-ir/target/release/shader-ir"
SHADERS = LAB / "shaders"
SCENARIOS = LAB / "scenarios"
VARIANTS = LAB / "variants"
BUILD = LAB / "build"
SPV_DIR = BUILD / "spv"
INPUTS_DIR = BUILD / "inputs"
JOBS_DIR = BUILD / "jobs"
RESULTS_DIR = LAB / "results"
RESULTS_JSONL = LAB / "results.jsonl"
BUDGETS = LAB / "budgets.toml"

def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def run(cmd, **kw) -> subprocess.CompletedProcess:
    return subprocess.run([str(c) for c in cmd], capture_output=True, text=True, **kw)

def git_commit(repo: Path) -> str:
    r = run(["git", "-C", repo, "rev-parse", "HEAD"])
    return r.stdout.strip() if r.returncode == 0 else "unknown"

def tool_versions() -> dict:
    v = {}
    r = run([GLSLANG, "--version"]); v["glslang"] = r.stdout.splitlines()[0].strip() if r.returncode == 0 else "missing"
    r = run([SPIRV_VAL, "--version"]); v["spirv-tools"] = r.stdout.strip() if r.returncode == 0 else "missing"
    v["igl_commit"] = git_commit(IGL)
    v["lab_commit"] = git_commit(ROOT)
    v["runner"] = sha256_file(RUNNER)[:16] if RUNNER.exists() else "missing"
    v["shader_ir"] = sha256_file(SHADER_IR)[:16] if SHADER_IR.exists() else "missing"
    v["os"] = f"{platform.system()} {platform.release()}"
    return v

def require(p: Path, what: str):
    if not p.exists():
        raise SystemExit(f"missing {what}: {p}")
