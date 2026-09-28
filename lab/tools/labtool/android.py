"""adb-driven execution of job bundles on an Android device.

The headless runner (lab/runner/build-android/shaderlab-runner, built by lab/runner/build-android.sh) lives in
/data/local/tmp/shaderlab/ on the device and is driven entirely through adb: push the job bundle, run, pull the
result bundle, delete the job. Inputs are cached on the device by content hash (CONTRACTS.md "Android runner").
"""
from __future__ import annotations
import json, os, re, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from .paths import LAB, sha256_file

ADB = Path(os.environ.get("LAB_ADB", str(Path.home() / "Android/platform-tools/adb")))
RUNNER_ANDROID = LAB / "runner/build-android/shaderlab-runner"
DEVICE_ROOT = "/data/local/tmp/shaderlab"
DEVICE_RUNNER = f"{DEVICE_ROOT}/shaderlab-runner"
DEVICE_INPUTS = f"{DEVICE_ROOT}/inputs"
DEVICE_JOBS = f"{DEVICE_ROOT}/jobs"
# android.os.PowerManager.THERMAL_STATUS_*; the runner writes the name in state.thermal and the number in state.thermal_status
THERMAL_STATUS = {"none": 0, "light": 1, "moderate": 2, "severe": 3, "critical": 4, "emergency": 5, "shutdown": 6}
THROTTLE_AT = THERMAL_STATUS["severe"]   # a sample taken at or above this status is retried, then marked throttled
COOLDOWN_S = 30
MAX_RETRIES = 3

class AdbError(RuntimeError):
    pass

_runner_ok: set[str] = set()          # serials whose runner was verified this process
_inputs_ok: dict[str, set[str]] = {}  # serial -> remote input paths verified this process
_sha_cache: dict[tuple, str] = {}     # (path, size, mtime) -> sha256

def _adb_base(serial: str | None) -> list[str]:
    if not ADB.exists():
        raise AdbError(f"adb not found at {ADB} (set LAB_ADB)")
    return [str(ADB)] + (["-s", serial] if serial else [])

def adb(serial: str | None, *args, check: bool = True, timeout: int = 600) -> subprocess.CompletedProcess:
    r = subprocess.run(_adb_base(serial) + [str(a) for a in args], capture_output=True, text=True, timeout=timeout)
    if check and r.returncode != 0:
        raise AdbError(f"adb {' '.join(str(a) for a in args)} failed ({r.returncode}): {(r.stderr or r.stdout).strip()}")
    return r

def shell(serial: str, cmd: str, check: bool = True, timeout: int = 600) -> tuple[int, str]:
    """Run a shell command on the device; the remote exit code is captured explicitly (old adb servers drop it)."""
    r = adb(serial, "shell", f"({cmd}); echo __rc=$?", check=False, timeout=timeout)
    if r.returncode != 0:
        raise AdbError(f"adb shell failed: {(r.stderr or r.stdout).strip()}")
    m = re.search(r"__rc=(\d+)\s*$", r.stdout)
    rc = int(m.group(1)) if m else r.returncode
    out = r.stdout[: m.start()] if m else r.stdout
    if check and rc != 0:
        raise AdbError(f"device command failed ({rc}): {cmd}\n{out.strip()}")
    return rc, out

def list_devices() -> list[dict]:
    """`adb devices -l`: [{serial, state, model, product, transport_id}]."""
    r = adb(None, "devices", "-l")
    out = []
    for line in r.stdout.splitlines()[1:]:
        parts = line.split()
        if len(parts) < 2:
            continue
        d = {"serial": parts[0], "state": parts[1]}
        for kv in parts[2:]:
            if ":" in kv:
                k, v = kv.split(":", 1); d[k] = v
        out.append(d)
    return out

def pick_serial(serial: str | None) -> str:
    if serial:
        return serial
    ready = [d for d in list_devices() if d["state"] == "device"]
    if len(ready) == 1:
        return ready[0]["serial"]
    if not ready:
        states = ", ".join(f"{d['serial']}={d['state']}" for d in list_devices()) or "none attached"
        raise AdbError(f"no authorized device ({states}); accept the USB-debugging prompt on the phone or pass --android SERIAL")
    raise AdbError("several devices attached; pass --android SERIAL: " + ", ".join(d["serial"] for d in ready))

def device_props(serial: str) -> dict:
    keys = ["ro.product.model", "ro.product.manufacturer", "ro.build.version.release", "ro.build.version.sdk",
            "ro.hardware", "ro.board.platform", "ro.build.fingerprint"]
    _, out = shell(serial, " ; ".join(f"echo {k}=$(getprop {k})" for k in keys))
    props = {}
    for line in out.splitlines():
        if "=" in line:
            k, v = line.split("=", 1); props[k.strip()] = v.strip()
    return props

def push_runner(serial: str, force: bool = False) -> dict:
    """Push the arm64 runner once; skipped when the device already holds a binary with the same sha256."""
    if not RUNNER_ANDROID.exists():
        raise AdbError(f"Android runner not built: {RUNNER_ANDROID} (run lab/runner/build-android.sh)")
    local_sha = sha256_file(RUNNER_ANDROID)
    shell(serial, f"mkdir -p {DEVICE_INPUTS} {DEVICE_JOBS}")
    rc, out = shell(serial, f"sha256sum {DEVICE_RUNNER} 2>/dev/null", check=False)
    remote_sha = out.split()[0] if rc == 0 and out.split() else None
    pushed = False
    if force or remote_sha != local_sha:
        adb(serial, "push", RUNNER_ANDROID, DEVICE_RUNNER)
        shell(serial, f"chmod 755 {DEVICE_RUNNER}")
        pushed = True
    _runner_ok.add(serial)
    return {"pushed": pushed, "sha256": local_sha, "remote": DEVICE_RUNNER}

def _ensure_runner(serial: str) -> None:
    if serial not in _runner_ok:
        push_runner(serial)

def runner_info(serial: str, device_index: int | None = None, validation: bool = False) -> dict:
    """`shaderlab-runner --info` on the device: Vulkan device properties, device list, thermal/battery state."""
    _ensure_runner(serial)
    cmd = f"{DEVICE_RUNNER} --info" + ("" if validation else " --no-validation") + (f" --device {device_index}" if device_index is not None else "")
    rc, out = shell(serial, cmd, check=False, timeout=120)
    start = out.find("{")
    if start < 0:
        raise AdbError(f"runner --info produced no JSON (exit {rc}):\n{out.strip()[:2000]}")
    try:
        return json.loads(out[start:])
    except json.JSONDecodeError as e:
        raise AdbError(f"runner --info JSON parse error: {e}\n{out[start:start+500]}")

def _sha(p: Path) -> str:
    st = p.stat(); key = (str(p), st.st_size, st.st_mtime_ns)
    if key not in _sha_cache:
        _sha_cache[key] = sha256_file(p)
    return _sha_cache[key]

def ensure_input(serial: str, local: Path) -> str:
    """Content-addressed input cache on the device: /data/local/tmp/shaderlab/inputs/<sha256>.npy. Returns the remote path."""
    local = local.resolve()
    remote = f"{DEVICE_INPUTS}/{_sha(local)}.npy"
    seen = _inputs_ok.setdefault(serial, set())
    if remote in seen:
        return remote
    rc, out = shell(serial, f"stat -c %s {remote} 2>/dev/null", check=False)
    if rc == 0 and out.strip().isdigit() and int(out.strip()) == local.stat().st_size:
        seen.add(remote); return remote
    shell(serial, f"mkdir -p {DEVICE_INPUTS}")
    adb(serial, "push", local, remote + ".part", timeout=1800)   # 33 MB per 1080p image
    shell(serial, f"mv {remote}.part {remote}")
    seen.add(remote)
    return remote

def thermal_level(state: dict | None) -> int | None:
    """Android ThermalStatus 0..6 from a result's state object (None when not readable / not Android)."""
    if not isinstance(state, dict):
        return None
    ts = state.get("thermal_status")
    if isinstance(ts, int):
        return ts
    return THERMAL_STATUS.get(str(state.get("thermal", "")).lower())

def _pull_dir(serial: str, remote: str, out_dir: Path) -> None:
    """adb pull <remote dir> into out_dir (flattening adb's <out_dir>/<basename> convention)."""
    with tempfile.TemporaryDirectory(prefix="labpull-") as td:
        adb(serial, "pull", remote, td, timeout=1800)
        src = Path(td) / Path(remote).name
        if not src.is_dir():
            src = Path(td)
        for f in src.iterdir():
            shutil.move(str(f), str(out_dir / f.name))

def run_job_android(job_dir: Path, out_dir: Path, serial: str, device_index: int | None = None, validation: bool = False,
                    retries: int = MAX_RETRIES, cooldown_s: float = COOLDOWN_S, log=print) -> subprocess.CompletedProcess:
    """Push the job bundle, run shaderlab-runner on the device, pull the result bundle into out_dir, delete the job.

    Thermal gate (AXIOM_SHADER_LAB.md §3): a run whose state (before or after) reports ThermalStatus >= SEVERE is
    retried after `cooldown_s` up to `retries` times; the final result carries state.throttled=true/false so the
    stats layer can drop the sample. Validation layers are not available to a /data/local/tmp binary on production
    devices, so validation is off unless asked. Returns a CompletedProcess-like object (stdout/stderr of the runner).
    """
    job_dir = Path(job_dir); out_dir = Path(out_dir); out_dir.mkdir(parents=True, exist_ok=True)
    _ensure_runner(serial)
    job = json.loads((job_dir / "job.json").read_text())
    stamp = time.strftime("%Y%m%d-%H%M%S")
    safe = re.sub(r"[^A-Za-z0-9_.-]", "_", f"{job_dir.parent.name}-{job_dir.name}-{stamp}-{os.getpid()}")
    remote_job = f"{DEVICE_JOBS}/{safe}"
    shell(serial, f"rm -rf {remote_job} && mkdir -p {remote_job}/spv {remote_job}/result")
    # The pushed bundle is self-contained whatever the local layout: spv/<pass>.spv, scenario.toml, job.json with
    # absolute paths into the device's input cache (job.json paths may be absolute, CONTRACTS.md).
    for pname, rel in list(job["passes"].items()):
        adb(serial, "push", job_dir / rel, f"{remote_job}/spv/{pname}.spv", timeout=600)
        job["passes"][pname] = f"spv/{pname}.spv"
    adb(serial, "push", job_dir / job["scenario"], f"{remote_job}/scenario.toml")
    job["scenario"] = "scenario.toml"
    for name, rel in list(job.get("inputs", {}).items()):
        job["inputs"][name] = ensure_input(serial, job_dir / rel)
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as tf:
        json.dump(job, tf, indent=2); tmp_job = tf.name
    try:
        adb(serial, "push", tmp_job, f"{remote_job}/job.json")
    finally:
        os.unlink(tmp_job)
    cmd = f"cd {remote_job} && {DEVICE_RUNNER} --job {remote_job}/job.json --out {remote_job}/result"
    if not validation:
        cmd += " --no-validation"
    if device_index is not None:
        cmd += f" --device {device_index}"
    rc, out, result, attempts = 1, "", None, 0
    try:
        for attempt in range(retries + 1):
            attempts = attempt + 1
            if attempt:
                shell(serial, f"rm -rf {remote_job}/result && mkdir -p {remote_job}/result")
                for f in out_dir.iterdir():
                    if f.suffix in (".npy", ".json"):
                        f.unlink()
            rc, out = shell(serial, cmd, check=False, timeout=3600)
            _pull_dir(serial, f"{remote_job}/result", out_dir)
            rj = out_dir / "result.json"
            result = json.loads(rj.read_text()) if rj.exists() else None
            if result is None:
                break
            level = max([l for l in (thermal_level(result.get("state")), thermal_level(result.get("state_after"))) if l is not None], default=None)
            if level is None or level < THROTTLE_AT:
                break
            log(f"android {serial}: thermal status {level} (>= {THROTTLE_AT} SEVERE) after attempt {attempts}; " +
                (f"cooling down {cooldown_s:.0f} s and retrying" if attempt < retries else "giving up, sample marked throttled"))
            if attempt < retries:
                time.sleep(cooldown_s)
        if result is not None:
            for key in ("state", "state_after"):
                if isinstance(result.get(key), dict):
                    lvl = thermal_level(result[key])
                    result[key]["throttled"] = bool(lvl is not None and lvl >= THROTTLE_AT)
            if isinstance(result.get("state"), dict):
                result["state"]["cooldown_retries"] = attempts - 1
                result["state"]["serial"] = serial
            (out_dir / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    finally:
        shell(serial, f"rm -rf {remote_job}", check=False)
    return subprocess.CompletedProcess(args=[DEVICE_RUNNER, "--job", remote_job], returncode=rc, stdout=out, stderr="")

def clean(serial: str, inputs: bool = False) -> None:
    shell(serial, f"rm -rf {DEVICE_JOBS}" + (f" {DEVICE_INPUTS}" if inputs else ""), check=False)
    _inputs_ok.pop(serial, None)

# ---- CLI: lab android ... ----

def cmd_devices(a) -> None:
    devs = list_devices()
    if not devs:
        print("no devices attached (adb devices -l is empty)"); return
    for d in devs:
        line = f"{d['serial']:20s} {d['state']:12s}"
        if d["state"] != "device":
            hint = {"unauthorized": "accept the USB-debugging prompt on the phone", "offline": "replug / adb kill-server",
                    "no": "udev permissions: add a rule for the vendor id and replug"}.get(d["state"], "")
            print(line + f" {d.get('model', '')} {hint}"); continue
        try:
            p = device_props(d["serial"])
            line += f" {p.get('ro.product.manufacturer', '')} {p.get('ro.product.model', '')} Android {p.get('ro.build.version.release', '?')} (sdk {p.get('ro.build.version.sdk', '?')}, {p.get('ro.board.platform') or p.get('ro.hardware', '')})"
        except AdbError as e:
            line += f" props: {e}"
        print(line)
        if a.no_probe:
            continue
        try:
            info = runner_info(d["serial"], device_index=a.device)
            dev = info.get("device") or {}
            st = info.get("state") or {}
            if info.get("ok"):
                print(f"{'':20s} vulkan: {dev.get('name')} driver {dev.get('driver')} api {dev.get('api')} timestamp_period {dev.get('timestamp_period_ns')} ns")
                print(f"{'':20s} devices: " + ", ".join(f"[{x['index']}] {x['name']} (type {x['type']})" for x in info.get("devices", [])))
            else:
                print(f"{'':20s} runner --info FAILED: {info.get('error')}")
            print(f"{'':20s} state: thermal {st.get('thermal')} (status {st.get('thermal_status')}) gpu clock {st.get('clocks_mhz', {}).get('gpu')} MHz "
                  f"battery {st.get('battery', {}).get('level')}% {st.get('battery', {}).get('status')} temp {st.get('temperature_c')} C")
        except AdbError as e:
            print(f"{'':20s} probe: {e}")

def cmd_push(a) -> None:
    serial = pick_serial(a.serial)
    r = push_runner(serial, force=a.force)
    print(f"{serial}: runner {'pushed' if r['pushed'] else 'already up to date'} at {r['remote']} (sha256 {r['sha256'][:16]})")

def cmd_probe(a) -> None:
    serial = pick_serial(a.serial)
    print(json.dumps(runner_info(serial, device_index=a.device, validation=a.validation), indent=2))

def cmd_clean(a) -> None:
    serial = pick_serial(a.serial)
    clean(serial, inputs=a.inputs)
    print(f"{serial}: removed {DEVICE_JOBS}" + (f" and {DEVICE_INPUTS}" if a.inputs else ""))

def add_parser(sub) -> None:
    p = sub.add_parser("android", help="adb-driven Android runner: devices, push, probe, clean (run jobs with `lab run --android SERIAL`)")
    s = p.add_subparsers(dest="android_cmd", required=True)
    q = s.add_parser("devices"); q.add_argument("--no-probe", action="store_true", help="skip the runner --info probe"); q.add_argument("--device", type=int, default=None); q.set_defaults(f=cmd_devices)
    q = s.add_parser("push"); q.add_argument("--serial"); q.add_argument("--force", action="store_true"); q.set_defaults(f=cmd_push)
    q = s.add_parser("probe"); q.add_argument("--serial"); q.add_argument("--device", type=int, default=None); q.add_argument("--validation", action="store_true"); q.set_defaults(f=cmd_probe)
    q = s.add_parser("clean"); q.add_argument("--serial"); q.add_argument("--inputs", action="store_true", help="also drop the input cache"); q.set_defaults(f=cmd_clean)
