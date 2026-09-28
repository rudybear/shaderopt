from __future__ import annotations
import json, os, shutil, time
from pathlib import Path
import numpy as np
from .paths import (INPUTS_DIR, JOBS_DIR, RESULTS_DIR, RUNNER, VARIANTS, run, require, tool_versions)
from .scenarios import Scenario
from .generators import generate
from .build import baseline_spv
from .images import to_png, to_exr
from .formats import LDR_FORMATS

def gen_inputs(sc: Scenario, force: bool = False) -> dict[str, Path]:
    out = {}
    d = INPUTS_DIR / sc.name; d.mkdir(parents=True, exist_ok=True)
    for inp in sc.inputs:
        p = d / f"{inp['name']}.npy"
        if force or not p.exists():
            if "file" in inp and inp["file"].startswith("__extra__/"):
                continue  # supplied by make_job(extra_inputs=...)
            if "file" in inp:
                src = (sc.path.parent / inp["file"]).resolve()
                img = np.load(src).astype(np.float32)
                if img.shape[:2] != (sc.height, sc.width):
                    raise SystemExit(f"input {src} is {img.shape[:2]}, scenario is {(sc.height, sc.width)}")
            else:
                img = generate(inp["generator"], sc.width, sc.height, int(inp.get("seed", 0)), inp.get("params", {}))
            np.save(p, img)
        out[inp["name"]] = p
    return out

def scenario_toml(sc: Scenario) -> str:
    import toml
    d = {"scenario": {"name": sc.name, "split": sc.split, "width": sc.width, "height": sc.height},
         "inputs": [dict(i) for i in sc.inputs],
         "passes": [{"name": p.name, "shader": p.shader, "samplers": p.samplers, "uniforms": p.uniforms,
                     "output": {"format": p.format, "scale": p.scale}, "load": p.load, "store": p.store, "sampler": p.sampler} for p in sc.passes],
         "quality": {"outputs": sc.quality_outputs}}
    return toml.dumps(d)

def variant_spv(shader: str, variant_id: str) -> Path:
    if variant_id == "baseline":
        return baseline_spv(shader)
    p = VARIANTS / shader / variant_id / f"{shader}.spv"
    if not p.exists():
        raise SystemExit(f"variant spv not found: {p}")
    return p

def make_job(sc: Scenario, variant_id: str = "baseline", per_pass_variant: dict | None = None, samples: int = 30,
             iterations: int = 8, warmup: int = 5, readback: str = "last", tag: str | None = None, extra_inputs: dict | None = None) -> Path:
    """per_pass_variant maps pass name -> variant id (default: variant_id for every pass whose shader has it, else baseline)."""
    inputs = gen_inputs(sc)
    jid = tag or variant_id
    jd = JOBS_DIR / sc.name / jid
    if jd.exists():
        shutil.rmtree(jd)
    (jd / "spv").mkdir(parents=True); (jd / "inputs").mkdir()
    passes = {}
    for p in sc.passes:
        vid = (per_pass_variant or {}).get(p.name, variant_id)
        if vid != "baseline" and not (VARIANTS / p.shader / vid).exists():
            vid = "baseline"
        src = variant_spv(p.shader, vid)
        dst = jd / "spv" / f"{p.name}.spv"; shutil.copyfile(src, dst)
        passes[p.name] = f"spv/{p.name}.spv"
    ins = {}
    for name, p in inputs.items():
        dst = jd / "inputs" / f"{name}.npy"
        os.symlink(p.resolve(), dst)
        ins[name] = f"inputs/{name}.npy"
    for name, arr in (extra_inputs or {}).items():
        dst = jd / "inputs" / f"{name}.npy"; np.save(dst, np.asarray(arr, dtype=np.float32)); ins[name] = f"inputs/{name}.npy"
    # Always serialize the Scenario object we were given: derived scenarios (formats, resolution, extra inputs) must reach the runner.
    (jd / "scenario.toml").write_text(scenario_toml(sc))
    job = {"schema": 1, "scenario": "scenario.toml", "variant_id": variant_id, "passes": passes,
           "inputs": ins, "samples": samples, "iterations": iterations, "warmup": warmup, "readback": readback}
    (jd / "job.json").write_text(json.dumps(job, indent=2))
    return jd

def run_job(job_dir: Path, sc: Scenario, variant_id: str, out_dir: Path | None = None, device: int | None = None,
            android: str | None = None) -> tuple[dict, Path]:
    """Run a job bundle on the desktop runner, or on an Android device (adb serial) through labtool.android."""
    stamp = time.strftime("%Y%m%d-%H%M%S")
    out = out_dir or (RESULTS_DIR / sc.name / variant_id / stamp)
    out.mkdir(parents=True, exist_ok=True)
    if android:
        from .android import run_job_android
        r = run_job_android(job_dir, out, android, device_index=device)
    else:
        require(RUNNER, "runner binary (build lab/runner first)")
        cmd = [RUNNER, "--job", job_dir / "job.json", "--out", out]
        if device is not None:
            cmd += ["--device", str(device)]
        r = run(cmd)
    (out / "runner.stdout").write_text(r.stdout); (out / "runner.stderr").write_text(r.stderr)
    rj = out / "result.json"
    if not rj.exists():
        raise SystemExit(f"runner produced no result.json (exit {r.returncode}); see {out}/runner.stderr")
    res = json.loads(rj.read_text())
    res["_dir"] = str(out)
    res["_tools"] = tool_versions()
    if android:
        res["_android"] = android
    if res.get("ok"):
        for pname, rel in res.get("images", {}).items():
            img = np.load(out / rel)
            fmt = next(p.format for p in sc.passes if p.name == pname)
            to_png(img, out / f"{pname}.png", hdr=fmt not in LDR_FORMATS)
            if os.environ.get("LAB_WRITE_EXR"):
                try:
                    to_exr(img, out / f"{pname}.exr")
                except Exception as e:  # EXR is for humans; never fail a run on it
                    (out / f"{pname}.exr.error").write_text(str(e))
    return res, out

def drop_images(*results: dict) -> None:
    """Delete the raw image dumps of finished runs (they are reproducible and 33 MB each at 1080p); PNGs stay."""
    for res in results:
        d = Path(res.get("_dir", ""))
        if d.is_dir():
            for f in list(d.glob("*.npy")) + list(d.glob("*.exr")):
                f.unlink(missing_ok=True)

def load_result_images(res: dict) -> dict[str, np.ndarray]:
    d = Path(res["_dir"])
    return {k: np.load(d / v) for k, v in res.get("images", {}).items()}
