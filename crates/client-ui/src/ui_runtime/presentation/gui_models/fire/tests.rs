use std::sync::Arc;
use {super::*, ui::IconRef};

fn texture() -> ParticleTexture {
    ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: 16,
        height: 32,
        rgba8: (0..16 * 32)
            .flat_map(|pixel| [pixel as u8, (pixel / 16) as u8, 0, 255])
            .collect::<Vec<_>>()
            .into(),
    }
}

#[test]
fn flame_frames_retain_original_texels_and_animate_without_texture_rebuilds() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    let source = texture();
    presentation.set_gui_fire_texture(Some(&source));
    let textures = Arc::clone(&presentation.textures);
    let frames = presentation.gui_models.fire.frames.clone();
    assert_eq!(frames.len(), 2);
    for (frame, icon) in frames.iter().enumerate() {
        let page = &textures.pages()[usize::from(icon.page)];
        let stride = page.dimensions()[0] as usize;
        for row in 0..16 {
            let start = ((usize::from(icon.uv[1]) + row) * stride + usize::from(icon.uv[0])) * 4;
            let original = (frame * 16 * 16 + row * 16) * 4;
            assert_eq!(
                &page.pixels()[start..start + 16 * 4],
                &source.rgba8[original..original + 16 * 4]
            );
        }
    }
    presentation.player_preview_view = player_preview::PreviewView::Hud;
    presentation.gui_models.live_player.fire_size = Some([0.6, 1.8]);
    presentation.gui_models.live_player.outer_y = player_preview::HUD_SWIM_OFFSET;
    for frame in [1, 1, 0, 0, 1] {
        presentation
            .gui_models
            .live_player
            .fire
            .observe((1, 1, 1), true, frames.len());
        assert_eq!(
            presentation
                .gui_fire_frame(presentation.player_preview_view)
                .unwrap()
                .texture,
            frames[frame]
        );
        assert_eq!(
            presentation
                .gui_fire_frame(presentation.player_preview_view)
                .unwrap()
                .outer_y,
            player_preview::HUD_SWIM_OFFSET
        );
    }
    assert!(Arc::ptr_eq(&textures, &presentation.textures));
    presentation.gui_models.live_player.fire_size = None;
    assert!(
        presentation
            .gui_fire_frame(presentation.player_preview_view)
            .is_none()
    );
    presentation.gui_models.live_player.fire_size = Some([0.6, 1.8]);
    presentation.player_preview_view = player_preview::PreviewView::default();
    assert!(
        presentation
            .gui_fire_frame(presentation.player_preview_view)
            .is_none()
    );
}

#[test]
fn optional_flame_texture_can_be_replaced_removed_or_rejected_without_accumulating_pages() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut texture = texture();
    presentation.set_gui_fire_texture(Some(&texture));
    let pages = presentation.gui_models.sources.pages.len();
    texture.rgba8 = vec![255; texture.rgba8.len()].into();
    presentation.set_gui_fire_texture(Some(&texture));
    assert_eq!(presentation.gui_models.sources.pages.len(), pages);
    assert_eq!(presentation.gui_models.fire.frames.len(), 2);
    texture.height = 17;
    presentation.set_gui_fire_texture(Some(&texture));
    assert!(presentation.gui_models.sources.pages.is_empty());
    assert!(presentation.gui_models.fire.frames.is_empty());
    presentation.set_gui_fire_texture(None);
    assert!(presentation.gui_models.sources.pages.is_empty());
}

#[test]
fn fire_playback_holds_when_extinguished_and_resets_when_actor_is_replaced() {
    let mut playback = FirePlayback::default();
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), false, 32);
    playback.observe((1, 1, 1), false, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 1);
    playback.observe((1, 1, 1), true, 32);
    assert_eq!(playback.frame, 2);
    playback.observe((1, 1, 2), true, 32);
    assert_eq!(playback.frame, 1);
}

#[test]
fn review_actor_flames_reclaim_pages_from_optional_block_models() {
    use super::super::{GuiModels, PreparedGuiModels, icon_key, item_gui};
    use render_model::UiTexturePage;
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE;
    let page =
        UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap();
    let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
    let required = IconRef {
        color_mask: None,
        page: first,
        uv: [1, 1, 17, 17],
        glint: false,
    };
    let optional = IconRef {
        page: first + 1,
        ..required
    };
    let required_mesh = item_gui::cube([required; 6], Default::default()).unwrap();
    let optional_mesh = item_gui::cube([optional; 6], Default::default()).unwrap();
    presentation.gui_models = GuiModels {
        enabled: true,
        sources: PreparedGuiModels {
            pages: vec![page; MODEL_PAGES],
            required_pages: 1,
            models: [
                (icon_key(required), Arc::clone(&required_mesh)),
                (icon_key(optional), optional_mesh),
            ]
            .into(),
            optional_models: [icon_key(optional)].into(),
            textures: [
                (super::super::atlas::key([1; 2], &[1; 4]), required),
                (super::super::atlas::key([1; 2], &[2; 4]), optional),
            ]
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let source = texture();
    presentation.set_gui_fire_texture(Some(&source));
    assert_eq!(presentation.gui_models.fire.frames.len(), 2);
    assert_eq!(presentation.gui_models.sources.pages.len(), 2);
    assert!(Arc::ptr_eq(
        &presentation.gui_models.sources.models[&icon_key(required)],
        &required_mesh
    ));
    assert!(
        !presentation
            .gui_models
            .sources
            .models
            .contains_key(&icon_key(optional))
    );
    assert!(
        !presentation
            .gui_models
            .sources
            .textures
            .values()
            .any(|icon| icon.page == optional.page)
    );
    let frame = presentation.gui_models.fire.frames[0];
    let page = &presentation.textures.pages()[usize::from(frame.page)];
    let start = (usize::from(frame.uv[1]) * side as usize + usize::from(frame.uv[0])) * 4;
    assert_eq!(&page.pixels()[start..start + 4], &source.rgba8[..4]);
    let frame_side = side / 2;
    let large = ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: frame_side,
        height: frame_side * 4,
        rgba8: (0..4)
            .flat_map(|frame| [frame, 20, 40, 255].repeat((frame_side * frame_side) as usize))
            .collect::<Vec<_>>()
            .into(),
    };
    presentation.set_gui_fire_texture(Some(&large));
    assert_eq!(presentation.gui_models.fire.frames.len(), 4);
    assert_eq!(presentation.gui_models.sources.pages.len(), 5);
    presentation.set_gui_fire_texture(None);
    assert_eq!(presentation.gui_models.sources.pages.len(), 1);
}

#[test]
fn exhausted_required_pages_clear_previous_flame_frame_addresses() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.set_gui_fire_texture(Some(&texture()));
    assert!(!presentation.gui_models.fire.frames.is_empty());
    let page = presentation.gui_models.sources.pages[0].clone();
    presentation.gui_models.sources.pages = vec![page; MODEL_PAGES];
    presentation.gui_models.fire.pages_start = None;
    presentation.install_gui_fire();
    assert!(presentation.gui_models.fire.frames.is_empty());
}

// Local-only: installed item artwork fits without sprite fallbacks and leaves room for the
// installed actor flame, as at startup.
#[test]
fn installed_item_models_leave_room_for_installed_actor_flames() {
    use crate::test_support::fixture_hud;
    use assets::{RuntimeEquipmentCatalog, RuntimeIconCatalog, carriers};
    let compiled = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(carriers::COMPILED_DIR);
    let read = |carrier: &carriers::Carrier| {
        let path = compiled.join(carrier.output);
        std::fs::read(&path).map_err(|_| {
            eprintln!(
                "skipping installed_item_models_leave_room_for_installed_actor_flames: missing {}; run make assets",
                path.display()
            );
        })
    };
    let (Ok(world), Ok(entities), Ok(icons), Ok(equipment), Ok(particles)) = (
        read(&carriers::WORLD),
        read(&carriers::ENTITY),
        read(&carriers::ICON),
        read(&carriers::EQUIPMENT),
        read(&carriers::PARTICLE),
    ) else {
        return;
    };
    let world = assets::RuntimeAssets::decode(&world).unwrap();
    let entities = assets::RuntimeEntityAssets::decode(&entities).unwrap();
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).unwrap());
    let particles = assets::RuntimeParticleAssets::decode(&particles).unwrap();
    let flame = particles.texture(assets::ACTOR_FLAME_TEXTURE).unwrap();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        fixture_hud(),
        icons,
    )
    .unwrap();
    presentation.set_equipment_catalog(Some(Arc::new(
        RuntimeEquipmentCatalog::decode(&equipment).unwrap(),
    )));
    presentation.set_gui_models(&world, &entities).unwrap();
    assert_eq!(
        presentation.gui_models.sources.fallbacks,
        Default::default(),
        "installed item artwork must fit the model atlas"
    );
    presentation.set_gui_fire_texture(Some(flame));
    assert_eq!(
        presentation.gui_models.fire.frames.len(),
        (flame.height / flame.width) as usize
    );
}
