//! Installed-pack armor through the same trim cache and player model used in game.
use super::*;

/// Returns a fixed anonymous armor preview and its four stack icons for offline captures.
pub fn trim_gallery(
    catalog: Arc<assets::RuntimeEquipmentCatalog>,
    icons: Arc<assets::RuntimeIconCatalog>,
    trimmed: bool,
) -> (Arc<UiMesh>, Vec<(u16, UiTexturePage)>, Vec<IconRef>) {
    let mut runtime =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    runtime.set_equipment_catalog(Some(catalog));
    runtime.gui_models.enabled = true;
    runtime.gui_models.skin = Some(
        UiTexturePage::owned(
            [render_model::STANDARD_SKIN_SIDE as u32; 2],
            render_model::default_actor_skin_rgba8().pixels().clone(),
        )
        .unwrap(),
    );
    let trim = protocol::ItemTrim {
        material: "gold".into(),
        pattern: "sentry".into(),
    };
    let identifiers = [
        "minecraft:diamond_helmet",
        "minecraft:diamond_chestplate",
        "minecraft:diamond_leggings",
        "minecraft:diamond_boots",
    ];
    runtime.gui_models.trim_icons.begin_frame();
    if trimmed {
        for id in identifiers {
            runtime.trim_icon(id, 0, &trim, None);
        }
    }
    runtime.set_preview_gear(
        identifiers.map(|id| Some((id, None, false, trimmed.then_some(&trim)))),
        [None, None],
        None,
    );
    runtime.finish_trim_art();
    // Install the ordinary armor sheets as the normal source-preparation worker does.
    let mut atlas = atlas::Atlas::new(
        (runtime.textures.dynamic_start() + MODEL_PAGE) as u16,
        MODEL_PAGES,
    );
    for texture in runtime.player_preview_gear.armor.iter().flatten() {
        let icon = atlas
            .insert([texture.width, texture.height], &texture.rgba)
            .unwrap();
        runtime.gui_models.sources.textures.insert(
            atlas::key([texture.width, texture.height], &texture.rgba),
            icon,
        );
    }
    runtime.gui_models.sources.pages = atlas.pages().unwrap();
    runtime.rebuild_dynamic_textures();
    let icon_refs = identifiers
        .into_iter()
        .map(|id| {
            if trimmed {
                runtime
                    .trim_icon(id, 0, &trim, None)
                    .expect("trim icon has pack artwork")
            } else {
                runtime.item_icon(id, 0).unwrap()
            }
        })
        .collect();
    let view = player_preview::PreviewView::Doll {
        yaw: 30.0,
        tilt: -10.0,
    };
    let mesh = runtime
        .project_gui_player_mesh(
            &runtime.gui_player_sources(view).unwrap(),
            [None, None],
            view,
        )
        .unwrap();
    let pages = runtime
        .textures
        .pages()
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, page)| (index as u16, page))
        .collect();
    (mesh, pages, icon_refs)
}
