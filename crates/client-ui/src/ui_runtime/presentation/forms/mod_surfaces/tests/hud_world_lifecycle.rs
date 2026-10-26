//! Retained host surfaces rendered through the production HUD presenter.

use super::mod_hud::{frame, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_api::drawing::{DrawingPermissions, DrawingSession};
use mod_host::{ModGrants, ModHost};
use mod_ui::{ActorAuthority, PanelUiOwner, ProjectedActor, PublishedHudSurface};
use serde_json::{Value, json};
use ui_engine::text_metrics::TextMetrics;

/// A guest that publishes on init and republishes only its anchor on each frame.
fn guest(anchor: Value, split: bool, session: DrawingSession) -> ModHost {
    let tag = json!({"tag":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
        "size":[20,10],"anchor_from":"top_left","anchor_to":"top_left","color":[1,0,0,1],
        "bindings":[{"binding_name":"#tag","binding_name_override":"#offset"},
            {"binding_name":"#tag_visible","binding_name_override":"#visible"}]}});
    let mut controls = vec![tag];
    if split {
        controls.push(json!({"other":{"type":"panel","size":[0,0]}}));
    }
    let surface = json!({"screen":"hud.root","document":json!({"namespace":"hud",
        "root":{"type":"panel","size":["100%","100%"],"controls":controls}}).to_string(),
        "bindings":{"#tag":{"project":anchor}}})
    .to_string();
    let patch = json!({"binding_changes":{"#tag":{"project":anchor}}}).to_string();
    let package = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mod-api/wit/extension.wit"
    ))
    .lines()
    .next()
    .unwrap()
    .trim_start_matches("package ")
    .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let escape = |text: &str| {
        text.as_bytes()
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    };
    let source = format!(
        r#"(component
        (import "{name}/hud@{version}" (instance $hud
            (export "set-surface" (func (param "json" string) (result (result (error string)))))
            (export "update-surface" (func (param "json" string) (result (result (error string)))))))
        (alias export $hud "set-surface" (func $set))
        (alias export $hud "update-surface" (func $update))
        (core module $mem (memory (export "memory") 4)
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 131072))
        (core instance $memory (instantiate $mem))
        (alias core export $memory "memory" (core memory $m))
        (alias core export $memory "realloc" (core func $realloc))
        (core func $lower-set (canon lower (func $set) (memory $m) (realloc $realloc)))
        (core func $lower-update (canon lower (func $update) (memory $m) (realloc $realloc)))
        (core module $code
            (import "host" "memory" (memory 4))
            (import "host" "set" (func $set (param i32 i32 i32)))
            (import "host" "update" (func $update (param i32 i32 i32)))
            (data (i32.const 0) "{surface_data}")
            (data (i32.const 65536) "{patch_data}")
            (func (export "init")
                i32.const 0 i32.const {surface_len} i32.const 98304 call $set
                i32.const 98304 i32.load8_u if unreachable end)
            (func (export "frame")
                i32.const 65536 i32.const {patch_len} i32.const 98304 call $update
                i32.const 98304 i32.load8_u if unreachable end))
        (core instance $host (export "memory" (memory $m))
            (export "set" (func $lower-set)) (export "update" (func $lower-update)))
        (core instance $run (instantiate $code (with "host" (instance $host))))
        (func (export "init") (canon lift (core func $run "init")))
        (func (export "frame") (canon lift (core func $run "frame"))))"#,
        surface_data = escape(&surface),
        patch_data = escape(&patch),
        surface_len = surface.len(),
        patch_len = patch.len()
    );
    ModHost::prepare_snapshot_in_session(
        std::path::Path::new("projected-hud.wat"),
        source.as_bytes(),
        ModGrants {
            hud: true,
            players: true,
            ..Default::default()
        },
        None,
        Some(session),
    )
    .unwrap()
}

/// Paints only the host's surface through the same presenter used for gameplay.
fn paint(
    p: &mut UiPresentationRuntime,
    host: &ModHost,
    camera: Option<[f32; 16]>,
    actor: bool,
) -> Option<[f32; 4]> {
    let (surface, revision) = host.hud_surface().unwrap();
    p.mod_ui_mut()
        .set_hud_surfaces(std::iter::once(PublishedHudSurface {
            owner: PanelUiOwner::Environment(0),
            revision,
            surface,
            authority: ActorAuthority {
                players: true,
                entities: false,
            },
        }));
    p.mod_ui_mut().set_hud_projection(camera, &|id| {
        (actor && id == 42).then_some(ProjectedActor {
            feet: [0., 0., -10.],
            player: true,
        })
    });
    let mut nodes = Vec::new();
    p.append_hud_surfaces(
        &player_state::PlayerState::new(1),
        &UiRuntime::new(1),
        &mut nodes,
        &mut 0,
        TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.).unwrap(), Some(2)),
        [1280., 720.],
    );
    ui::RetainedDraw::build(
        &nodes,
        ui_engine::engine::rect(0., 0., 1280., 720.).unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
    )
    .unwrap()
    .draw_list()
    .painted_bounds()
    .map(|b| [b.min().x(), b.min().y(), b.max().x(), b.max().y()])
}

#[test]
fn invalidated_world_labels_stay_hidden_until_guest_republication() {
    let camera = Some(
        glam::camera::rh::proj::directx::perspective(1.0, 16. / 9., 0.1, 1000.).to_cols_array(),
    );
    let session = DrawingSession {
        connection: 1,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    };
    let mut failures = Vec::new();
    for anchor in [json!({"world":[0,0,-10]}), json!({"actor":42})] {
        for split in [false, true] {
            for gap in [false, true] {
                let mut host = guest(anchor.clone(), split, session);
                let mut p = presentation();
                frame(&player_state::PlayerState::new(1), &mut p);
                assert_eq!(
                    paint(&mut p, &host, None, true),
                    None,
                    "initial camera absence"
                );
                assert_eq!(
                    paint(&mut p, &host, camera, true),
                    Some([639.5, 359.5, 680.5, 380.5])
                );
                assert_eq!(paint(&mut p, &host, None, true), None, "camera loss");
                if anchor.get("actor").is_some() {
                    assert_eq!(paint(&mut p, &host, camera, false), None, "actor despawn");
                }
                assert!(paint(&mut p, &host, camera, true).is_some());
                if gap {
                    host.bind_drawing_world(session, false);
                    for republish in [false, true] {
                        if republish {
                            host.frame(false).unwrap();
                        }
                        let bounds = paint(&mut p, &host, None, true);
                        if bounds.is_some() {
                            failures.push(format!(
                                "gap {anchor} split={split} republish={republish}: {bounds:?}"
                            ));
                        }
                    }
                }
                host.bind_drawing_session(DrawingSession {
                    dimension: 1,
                    ..session
                });
                let bounds = paint(&mut p, &host, camera, true);
                if bounds.is_some() {
                    failures.push(format!(
                        "destination {anchor} split={split} gap={gap}: {bounds:?}"
                    ));
                }
                host.frame(false).unwrap();
                assert_eq!(
                    paint(&mut p, &host, camera, true),
                    Some([639.5, 359.5, 680.5, 380.5]),
                    "fresh destination anchor"
                );
                assert!(host.is_active());
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
