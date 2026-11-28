# Instruction budgets

Instruction budgets are the CI gate against CPU regressions. A test whose name starts with
`instruction_budget_` warms its scenario, then wraps the steady work in
`test_instructions::count`. `devtool instruction-budgets` runs each such test under Callgrind,
counting only what the calling thread retires inside that region, and compares the count with
`tools/devtool/instruction-budgets.json`. A count above **or below** its budget by more than
`TOLERANCE_PERCENT` in `tools/devtool/src/instruction_budgets.rs` fails,
as do a test without a budget, a budget without a test and a toolchain change, so gains are
locked in rather than left as headroom. For a given toolchain and build profile, repeated CI
runs agree to well within the tolerance; the tool pins glibc's CPU-dependent copy strategies so
CI's mixed runner CPUs agree too.

```sh
cargo run -p devtool --locked -- instruction-budgets            # check
cargo run -p devtool --locked -- instruction-budgets --update   # record changed budgets
```

It needs x86-64 Linux, Valgrind and cargo-nextest, and builds every workspace test binary, so run
it on a build host. It builds with CI's test-profile settings whatever the environment sets,
dropping `CARGO_PROFILE_*` and rustflags overrides, so a local update measures what CI does. CI runs it in
the Linux test job and uploads `instruction-budgets` with the measured file and one Callgrind
profile per test; when a change is intended, commit that file and explain increases in the PR.
Both it and `--update` keep each budget still within the tolerance and take the measurement only
for new, changed and removed tests, so repeated refreshes cannot absorb a slowdown step by step;
a toolchain change re-measures every budget.
`callgrind_annotate <test>.callgrind` attributes a change to functions.

Add a budget beside the code it protects for each hot path, especially per-frame work that a
settled frame should skip: keep the counted work single-threaded (make the test thread the only
Rayon worker when the path uses Rayon), deterministic and fast under
Callgrind (well under a second natively), assert the behaviour it measures, and record the new
entry with `--update`. Counts track work, not wall time; the hardware runs below remain the
evidence for frame-time claims.

# Headless chunk benchmarks

Criterion covers palette/column decode, full light solves, cube meshing, biome records,
bounded streaming bursts, dispatch-input capture, settled polling and metadata-only cohort
scans. Fixtures are synthetic and require no carriers, server, window or GPU.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --test
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --save-baseline chunks
```

Use the repository-pinned toolchain and the shared build-slot limiter described in
[the workflow](docs/agents/multi-agent-workflow.md). Run measurements without competing
builds or gameplay. Append a name filter, such as `stored_sections/871`, to select a
workload; subsequent runs can use `--baseline chunks` for comparison. Results stay
under the ignored `target/criterion/`.

The 871 fixture means **stored sections**, packed into 218 columns, not 871 columns.
Streaming setup preloads an implicit-air boundary through ordinary ingress, excluding
absent-neighbour grace waits from timing. Timed work includes bounded byte submission,
decode/commit, lighting, meshing, worker waits and CPU publication acknowledgements;
stream construction, boundary setup and teardown are excluded. Polling is unpaced.
Preflight CPU-step percentiles are not game-frame percentiles. Metadata scans have
no terrain. Resident-slot and stale-work counters include boundary setup; production
logs remain enabled. Decode and meshing timings include output destruction.

`pipeline/dispatch_inputs` captures 4/16/64/871 overlapping light or mesh inputs,
including handle release but excluding worker scheduling and execution. Its
`DISPATCH_INPUTS` records count allocations on the dispatch thread. The `_reused`
light cases retain worker scratch between solves; completed output remains owned.

`flight_costs` streams procedurally generated terrain (dirt over ore-flecked stone with
sealed caves) into a settled radius-10 or radius-16 view while the camera flies along +X.
Each 240 Hz frame submits the server's position, view centre and new columns, polls, and
acknowledges meshes. The flight cases report main-thread stream time per frame, and the
preflight line prints its p50, p99 and maximum. Eviction cases time only the server
position update that retires the trailing row. Cave cases time one full
connectivity search from the surface and from a sealed pocket.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench flight_costs
```

These are CPU baselines, not join-time or FPS evidence. They omit socket framing,
real server terrain, GPU preparation/uploads/draws and the rest of the Bevy frame.
Native performance acceptance still follows [live testing](docs/agents/live-testing.md).

# Scripted resident-render benchmark

`cinnabar-mcp --repo <checkout> --benchmark <plan.json> <new-output-directory>`
runs a developer-control client headlessly against a copied, pregenerated
[terrain fixture](../tools/localserver/terrain-fixture.md). It copies an isolated
user/cache template for each process, disables VSync, and rejects an effective
frame cap, scaled rendering, resized target or different server-granted radius.
Prepare the template with the desired explicit render distance and unlimited FPS.
Keep templates, worlds and output outside the checkout. Input paths are relative
to the plan file. The output directory must not already exist.

```json
{
  "binary": "bin/cinnabar",
  "server_binary": "bin/bedrock-local-server",
  "frozen_world": "terrain-template",
  "user_template": "benchmark-user-template",
  "machine_id": "local-machine-a",
  "power": "ac",
  "window": [1920, 1080],
  "render_distance": 12,
  "warmup_s": 2,
  "routes": [{
    "name": "flight",
    "start": [0, 100, 0],
    "end": [192, 100, 0],
    "yaw": -90,
    "pitch": 20,
    "vertical_fov": 70,
    "speed": 16
  }]
}
```

Choose positions inside the fixture's resident content. Each route holds its first
pose for settling, then flies linearly at the specified blocks per real second.
This camera-only workload leaves the player and stream anchor in place. Resident
column counts do not prove complete visible terrain or streaming performance.
The resident wait uses a square inscribed in the server's circular delivery radius;
it does not wait for undelivered square corners at the full requested radius.

The runner currently supports Linux and macOS host probes. `report.json` contains
exact average, 1% and 0.1% low FPS, frame-time percentiles and a fixed-edge histogram.
`manifest.json` links input and trace hashes, native CPU stage splits, sampled GPU
execution spans, GPU-completed title/world endpoints, sampled family peak RSS,
hardware and effective settings. CPU stages can overlap. GPU samples have receipt
times but no source-frame identity, so their all-frame tails remain unknown.
Startup times begin at process entry, and RSS sums can count shared pages twice.
Unsupported measurements stay explicit rather than becoming zero.

`--benchmark-compare <first/report.json> <second/report.json>` refuses missing or
different hardware, scene, route, cache, resolution, settings and timing semantics,
or missing phases. Its ratios are first/second and describe diagnostic interval
statistics only. One process run has no confidence interval. Full stage tracing
changes the workload; neither this report nor a matching comparison closes a
shipping-artifact, displayed-pacing, streaming, stability or hardware release gate.

Trace overwrites, missing frame sequences and unclosed final intervals fail capture.
Intervals belong to their starting frame, including a stall ending after the route.
Every attempt retains an outcome receipt; failed attempts must not be replaced by
successful retries. Logs and raw stage traces remain local and can contain private
session details. Reports are never uploaded automatically.
