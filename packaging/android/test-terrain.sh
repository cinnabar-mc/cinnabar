#!/usr/bin/env bash
# Runs the render crate's native terrain tests through ADB without app setup or game assets.
set -euo pipefail

if [[ $# != 3 ]]; then
    echo "Usage: $0 <Android render test binary> <adb serial> <output directory>" >&2
    exit 2
fi
binary=$1
serial=$2
out=$3
adb=${ADB:-adb}
[[ -f "$binary" ]] || { echo "Missing test binary: $binary" >&2; exit 2; }
command -v "$adb" >/dev/null
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [[ -e "$out/frames" || -e "$out/terrain.log" ]]; then
    echo "Use a fresh output directory so old captures cannot count as evidence." >&2
    exit 2
fi

remote=$("$adb" -s "$serial" shell mktemp -d /data/local/tmp/cinnabar-terrain.XXXXXX | tr -d '\r')
if [[ ! "$remote" =~ ^/data/local/tmp/cinnabar-terrain\.[a-zA-Z0-9]+$ ]]; then
    echo "Could not create a private device test directory." >&2
    exit 1
fi

# Removes only the device directory created by this invocation.
cleanup() {
    "$adb" -s "$serial" shell rm -rf "$remote" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

"$adb" -s "$serial" push "$binary" "$remote/terrain-test"
"$adb" -s "$serial" shell chmod 700 "$remote/terrain-test"
"$adb" -s "$serial" shell mkdir "$remote/frames"
set +e
"$adb" -s "$serial" shell \
    "cd '$remote' && CINNABAR_TERRAIN_CAPTURE_DIR='$remote/frames' ./terrain-test terrain_pixels_ --nocapture --test-threads=1" \
    2>&1 | tee "$out/terrain.log"
status=${PIPESTATUS[0]}
set -e
"$adb" -s "$serial" pull "$remote/frames" "$out/" || status=1
[[ $status == 0 ]] || exit "$status"

# A missing GPU fixture must fail a device run instead of passing through skipped tests.
python3 - "$out/frames" <<'PY'
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
views = set()
for path in root.glob("view*-indirectfalse.json"):
    metadata = json.loads(path.read_text())
    if metadata["backend"] != "Vulkan" or metadata["device_type"] == "Cpu":
        raise SystemExit("Terrain evidence must come from the device's Vulkan GPU.")
    if not path.with_suffix(".png").is_file():
        raise SystemExit("Missing completed terrain frame: " + path.stem)
    views.add(path.stem.split("-")[0])
if not {"view0", "view1"}.issubset(views):
    raise SystemExit("Missing native GPU terrain evidence for both camera directions.")
print("Verified completed terrain frames on the device's Vulkan GPU.")
PY
