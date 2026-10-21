#!/usr/bin/env bash
# render_smoke.sh: run the windowed game on a virtual X display with Mesa's
# software Vulkan driver (lavapipe) and fail on any render error.
#
# Bevy compiles WGSL only when it builds a pipeline. A shader that fails to
# compile is logged (`ERROR bevy_render::render_resource::pipeline_cache:
# failed to process shader`) while the game runs on without that mesh and
# exits 0, so no build, lint or unit test catches it. This does:
#   1. the game exits 0;
#   2. a PNG captured once the sim reaches a tick exists, so frames were
#      rendered with every pipeline the walkthrough's scene needs;
#   3. the log has no ERROR line.
#
# Needs xvfb (xvfb-run) and mesa-vulkan-drivers (lavapipe); CI's game-linux
# job installs both. Usage: scripts/render_smoke.sh <eonmark binary> [out-dir]
# (out-dir keeps the PNG, the log and the replay; default a temp dir).
set -euo pipefail

bin="${1:?usage: scripts/render_smoke.sh <eonmark binary> [out-dir]}"
out="${2:-$(mktemp -d)}"
# Ticks run in real time while frames take under 250 ms; lavapipe on a CI
# runner can be slower, so the tick is early and the deadline generous.
tick=100
seconds=60

icd=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1 || true)
if [ -z "$icd" ]; then
  echo "error: lavapipe not found (apt-get install mesa-vulkan-drivers)" >&2
  exit 1
fi
if ! command -v xvfb-run >/dev/null; then
  echo "error: xvfb-run not found (apt-get install xvfb)" >&2
  exit 1
fi

mkdir -p "$out"
png="$out/render_smoke.png"
log="$out/render_smoke.log"
rm -f "$png"

status=0
xvfb-run -a -s "-screen 0 1280x800x24" env VK_ICD_FILENAMES="$icd" \
  "$bin" --scenario economy_walkthrough --camera 38,64,60 \
  --exit-after-seconds "$seconds" --screenshot-at "$tick:$png" \
  --replay-dir "$out" >"$log" 2>&1 || status=$?

fail=0
if [ "$status" -ne 0 ]; then
  echo "error: the game exited $status" >&2
  fail=1
fi
if [ ! -s "$png" ]; then
  echo "error: no screenshot at tick $tick within $seconds s ($png)" >&2
  fail=1
fi
# Strip colour codes; tracing prints the level as a padded word.
errors=$(sed 's/\x1b\[[0-9;]*m//g' "$log" | grep -E '^[^ ]+ +ERROR ' || true)
if [ -n "$errors" ]; then
  echo "error: the log has ERROR lines:" >&2
  echo "$errors" >&2
  fail=1
fi
if [ "$fail" -ne 0 ]; then
  echo "--- $log (last 40 lines) ---" >&2
  tail -n 40 "$log" >&2
  exit 1
fi
echo "render_smoke: ok ($png)"
