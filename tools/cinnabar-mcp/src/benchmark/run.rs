//! Owned-process orchestration through existing developer-control routes.

use super::{Plan, files, observe, plan::resident_square_radius, report, trace};
use crate::processes::{Client, Launch, LocalServer};
use developer_control::{client::Controller, protocol::Command};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(250);

/// Runs one frozen fixture in disposable user/world directories and writes reduced local artifacts.
pub(super) fn run(repo: &Path, plan_file: &Path, output: &Path) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = parent.join(
        output
            .file_name()
            .ok_or("output needs a new directory name")?,
    );
    if output.starts_with(repo.canonicalize().map_err(|e| e.to_string())?) {
        return Err("benchmark output must be outside the checkout".into());
    }
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    files::write(
        &output.join("attempt.json"),
        &json!({"status": "started", "kind": "diagnostic_resident_render"}),
    )?;
    let result = run_plan(plan_file, &output);
    let outcome = match &result {
        Ok(()) => json!({"status": "complete"}),
        Err(error) => json!({"status": "failed_or_incomplete", "error": error}),
    };
    files::write(&output.join("outcome.json"), &outcome)?;
    result
}

/// Executes validated inputs while the outer attempt retains launch and capture failures.
fn run_plan(plan_file: &Path, output: &Path) -> Result<(), String> {
    let plan_file = plan_file.canonicalize().map_err(|e| e.to_string())?;
    let mut plan: Plan = files::read(&plan_file)?;
    let base = plan_file.parent().ok_or("plan has no parent directory")?;
    for path in [
        &mut plan.binary,
        &mut plan.server_binary,
        &mut plan.frozen_world,
        &mut plan.user_template,
    ] {
        *path = base
            .join(&*path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
    }
    if plan.machine_id.trim().is_empty()
        || !["ac", "battery"].contains(&plan.power.as_str())
        || !plan.warmup_s.is_finite()
        || plan.warmup_s < 2.
        || plan.routes.is_empty()
        || plan.window.contains(&0)
        || plan.render_distance == 0
    {
        return Err("plan needs machine identity, power, positive extent/radius, routes and at least two seconds settling".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for route in &plan.routes {
        route.path(plan.warmup_s)?;
        if route.name.is_empty() || !names.insert(&route.name) {
            return Err("route names must be nonempty and unique".into());
        }
    }
    if !plan.frozen_world.join("db").is_dir()
        || !plan.frozen_world.join("fixture-manifest.json").is_file()
    {
        return Err("frozen_world needs a pregenerated terrain fixture and its manifest".into());
    }
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let world = scratch.path().join("world");
    let user = scratch.path().join("user");
    let scene_hash = files::copy_tree(&plan.frozen_world, &world)?;
    let cache_hash = files::copy_tree(&plan.user_template, &user)?;
    let binary_hash = files::hash_file(&plan.binary)?;
    let server_hash = files::hash_file(&plan.server_binary)?;
    let hardware = observe::hardware()?;
    files::write(
        &output.join("inputs.json"),
        &json!({
            "binary_sha256": binary_hash, "server_sha256": server_hash, "scene_sha256": scene_hash,
            "user_template_sha256": cache_hash, "hardware": hardware, "machine_id": plan.machine_id,
            "power": plan.power, "window": plan.window, "render_distance": plan.render_distance,
            "warmup_s": plan.warmup_s, "routes": plan.routes,
        }),
    )?;
    let mut server = LocalServer::start(
        &plan.server_binary,
        &world,
        &[
            "-terrain-fixture".into(),
            "-game-mode".into(),
            "creative".into(),
            "-difficulty".into(),
            "peaceful".into(),
        ],
        output.join("server.log"),
    )?;
    let endpoint = scratch.path().join("endpoint.json");
    let trace_path = output.join("stages.json");
    let env = vec![
        (
            developer_control::ENDPOINT_ENV.into(),
            endpoint.display().to_string(),
        ),
        (developer_control::HIDDEN_WINDOW_ENV.into(), "1".into()),
        (
            developer_control::WINDOW_SIZE_ENV.into(),
            format!("{}x{}", plan.window[0], plan.window[1]),
        ),
        (developer_control::WINDOW_SCALE_ENV.into(), "1".into()),
        (render_api::USER_ROOT_ENV.into(), user.display().to_string()),
        (diagnostics::markers::STAGE_PROFILE.into(), "1".into()),
        (
            diagnostics::markers::STAGE_PROFILE_FRAMES.into(),
            trace_path.display().to_string(),
        ),
        (
            render_api::STAGE_PROFILE_EVENTS_ENV.into(),
            render_api::MAX_STAGE_PROFILE_EVENTS.to_string(),
        ),
        ("CINNABAR_LOG".into(), "join_profile=debug".into()),
    ];
    let args = vec!["--no-vsync".into()];
    let mut sampler = None;
    let (mut client, published) = Client::launch_observed(
        Launch {
            binary: &plan.binary,
            args: &args,
            env: &env,
            endpoint: &endpoint,
            log: output.join("client.log"),
            timeout: TIMEOUT,
        },
        |pid| sampler = Some(observe::Sampler::start(pid)),
    )?;
    let mut controller = Controller::connect_to(&published).map_err(|e| e.to_string())?;
    wait(&mut controller, |state| state["title_ms"].is_number())?;
    controller.call(
        &Command::Connect {
            address: server.address.clone(),
            auth_cache: None,
        },
        TIMEOUT,
    )?;
    let initial = wait(&mut controller, |state| state["world_ms"].is_number())?;
    validate_effective(&initial, &plan)?;
    let mut effective = Vec::new();
    for route in &plan.routes {
        let path = route.path(plan.warmup_s)?;
        let timeout = Duration::try_from_secs_f32(path.duration())
            .ok()
            .and_then(|duration| duration.checked_add(TIMEOUT))
            .ok_or("route duration exceeds the benchmark clock range")?;
        let timeout_ms = u64::try_from(timeout.as_millis())
            .map_err(|_| "route deadline exceeds the control clock range")?;
        // Resident counts are a precondition only; they do not prove visible-cohort completion.
        controller.call(
            &Command::WaitFor {
                condition: developer_control::protocol::Condition::ChunksLoaded {
                    radius: resident_square_radius(plan.render_distance),
                },
                timeout_ms: Some(TIMEOUT.as_millis() as u64),
            },
            TIMEOUT + Duration::from_secs(5),
        )?;
        controller.call(&Command::CameraPathRealTime(path), TIMEOUT)?;
        controller.call(
            &Command::WaitFor {
                condition: developer_control::protocol::Condition::CameraFinished,
                timeout_ms: Some(timeout_ms),
            },
            timeout
                .checked_add(Duration::from_secs(5))
                .ok_or("route reply deadline overflowed")?,
        )?;
        let state = controller.call(&Command::BenchmarkState, TIMEOUT)?;
        validate_effective(&state, &plan)?;
        effective.push(state);
    }
    controller.call(&Command::CameraRelease, TIMEOUT)?;
    controller.call(&Command::Quit, TIMEOUT)?;
    client.finish()?;
    let memory = sampler
        .take()
        .ok_or("memory sampler did not start")?
        .finish();
    server.stop();
    let trace: Value = files::read(&trace_path)?;
    let reduced = trace::reduce(&trace, &plan.routes, plan.warmup_s)?;
    report::write(
        output,
        &plan,
        &initial,
        &effective,
        hardware,
        memory,
        reduced,
        json!({"binary": binary_hash, "server": server_hash, "scene": scene_hash, "cache": cache_hash}),
    )
}

/// Polls read-only observations against a monotonic benchmark deadline.
fn wait(controller: &mut Controller, done: impl Fn(&Value) -> bool) -> Result<Value, String> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let state = controller.call(&Command::BenchmarkState, TIMEOUT)?;
        if done(&state) {
            return Ok(state);
        }
        if Instant::now() >= deadline {
            return Err("startup benchmark endpoint did not complete".into());
        }
        std::thread::sleep(POLL);
    }
}

/// Rejects caps, changed extents, scaling and negotiated radii that differ from the plan.
fn validate_effective(state: &Value, plan: &Plan) -> Result<(), String> {
    if state["window"]["physical"] != json!(plan.window)
        || state["window"]["hidden"] != true
        || state["video"]["render_scale_percent"] != 100
        || state["pacing"]["uncapped"] != true
        || state["pacing"]["display_sync"] != false
        || state["video"]["render_distance"] != plan.render_distance
        || state["render_distance_blocks"] != state["video"]["render_distance_blocks"]
        || state["gpu"]["name"].as_str().is_none_or(str::is_empty)
    {
        return Err(
            "effective extent, pacing, radius or GPU does not satisfy the benchmark plan".into(),
        );
    }
    Ok(())
}
