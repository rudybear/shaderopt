from __future__ import annotations
import json
from pathlib import Path
from .paths import GLSLANG, SPIRV_VAL, SHADERS, SPV_DIR, run, sha256_file, require, tool_versions

def compile_glsl(src: Path, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    r = run([GLSLANG, "-V", "-o", out, src])
    if r.returncode != 0:
        raise SystemExit(f"glslang failed for {src}:\n{r.stdout}{r.stderr}")
    r = run([SPIRV_VAL, out])
    if r.returncode != 0:
        raise SystemExit(f"spirv-val failed for {out}:\n{r.stdout}{r.stderr}")

def build_all() -> dict:
    require(GLSLANG, "pinned glslang"); require(SPIRV_VAL, "pinned spirv-val")
    manifest = {"tools": tool_versions(), "shaders": {}}
    for src in sorted(SHADERS.glob("*.frag")):
        out = SPV_DIR / f"{src.stem}.spv"
        compile_glsl(src, out)
        manifest["shaders"][src.stem] = {"src_sha256": sha256_file(src), "spv_sha256": sha256_file(out), "spv": str(out)}
    (SPV_DIR / "manifest.json").write_text(json.dumps(manifest, indent=2))
    return manifest

def baseline_spv(shader: str) -> Path:
    p = SPV_DIR / f"{shader}.spv"
    if not p.exists():
        build_all()
    return p
