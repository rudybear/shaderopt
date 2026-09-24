#!/usr/bin/env bash
# Builds the selftest shaders with the pinned glslang, validates them, runs the runner on the real
# GPU and checks the outputs with numpy. Usage: run_selftest.sh [extra runner args]
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER_DIR="$(dirname "$HERE")"
BUILD="$RUNNER_DIR/build"
RUNNER="$BUILD/shaderlab-runner"
WORK="$BUILD/selftest"
PY=/home/rudybear/sources/shaderopt/.venv/bin/python
GLSLANG=/home/rudybear/sources/igl/third-party/deps/src/glslang/build/StandAlone/glslang
SPIRV_VAL=/home/rudybear/sources/igl/third-party/deps/src/glslang/build/External/spirv-tools/tools/spirv-val
W=256; H=128
SAMPLES=5; ITER=4; WARMUP=2

EXTRA_ARGS=("$@")
[ -x "$RUNNER" ] || { echo "runner not built: $RUNNER (cmake -G Ninja -B $BUILD $RUNNER_DIR && ninja -C $BUILD)"; exit 1; }
mkdir -p "$WORK"

echo "== compile + validate shaders"
# NOTE: not -g0. glslang -g0 strips OpName, and the runner binds samplers and uniform members by
# name (CONTRACTS.md), so the SPIR-V must keep its debug names. The g0 case below proves the runner
# reports this clearly instead of guessing.
for s in copy copy_ubo; do
  "$GLSLANG" -V -o "$WORK/$s.spv" "$HERE/$s.frag"
  "$SPIRV_VAL" "$WORK/$s.spv" && echo "spirv-val ok: $s.spv"
done
"$GLSLANG" -V -g0 -o "$WORK/copy_g0.spv" "$HERE/copy.frag"
"$SPIRV_VAL" "$WORK/copy_g0.spv" && echo "spirv-val ok: copy_g0.spv (names stripped)"

echo "== input"
"$PY" "$HERE/gen_input.py" "$WORK/src.npy" $W $H

fail=0
run_case() { # name scenario spvname mode
  local name=$1 scenario=$2 spv=$3 mode=$4
  echo; echo "== case $name"
  "$PY" "$HERE/make_job.py" "$WORK/$name/job" "$HERE/$scenario" "$WORK/src.npy" "copy=$WORK/$spv.spv" \
      --samples $SAMPLES --iterations $ITER --warmup $WARMUP >/dev/null
  set +e
  "$RUNNER" --job "$WORK/$name/job/job.json" --out "$WORK/$name/result" "${EXTRA_ARGS[@]}"
  local rc=$?
  set -e
  echo "runner exit code: $rc"
  if ! "$PY" "$HERE/check.py" "$WORK/$name/result" "$WORK/src.npy" copy "$mode" --samples $SAMPLES; then fail=1; fi
}
run_case rgba32f passthrough.toml      copy     exact
run_case srgb    passthrough_srgb.toml copy     srgb
run_case ubo     passthrough_ubo.toml  copy_ubo ubo

echo; echo "== case g0 (negative: -g0 SPIR-V must fail with a clear message, exit 1, result.json ok=false)"
"$PY" "$HERE/make_job.py" "$WORK/g0/job" "$HERE/passthrough.toml" "$WORK/src.npy" "copy=$WORK/copy_g0.spv" >/dev/null
set +e; "$RUNNER" --job "$WORK/g0/job/job.json" --out "$WORK/g0/result" "${EXTRA_ARGS[@]}"; rc=$?; set -e
echo "runner exit code: $rc"
if [ $rc -eq 1 ] && "$PY" -c "import json,sys; r=json.load(open('$WORK/g0/result/result.json')); sys.exit(0 if (not r['ok'] and 'OpName' in r['error']) else 1)"; then
  echo "PASS (expected failure)"; else echo "FAIL: g0 case"; fail=1; fi

echo
if [ $fail -eq 0 ]; then echo "SELFTEST PASS"; else echo "SELFTEST FAIL"; exit 1; fi
