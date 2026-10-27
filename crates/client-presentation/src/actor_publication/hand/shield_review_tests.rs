//! Shared shield artwork must also reach the supported Java offhand route.
use super::*;

#[test]
fn java_offhand_shield_retains_its_banner_cloth() {
    let ((stream, mut equipment, artwork, mut input), entities) =
        fixture_with_appearance(Some(assets::gui_item::SHIELD_IDENTIFIER), false, false);
    let mut shield = input.main.take().unwrap();
    shield.shield_pattern = Some(Arc::new(render_api::ShieldPattern {
        base: 15,
        layers: Arc::from([]),
    }));
    input.off = Some(shield);
    let masks = assets::RuntimeEquipmentCatalog::from_parts(
        [8; 32],
        vec![],
        vec![assets::EquipmentTexture {
            identifier: assets::item::shield::pattern_path("").into(),
            width: 16,
            height: 16,
            rgba8: vec![255; 1024].into(),
        }],
    )
    .unwrap();
    equipment.set_pack_layer(Some((entities, Some(Arc::new(masks)), vec![])));
    let rig = stream.authority().actor_rig(1).unwrap();
    let actor = stream.authority().actor(1).unwrap();
    let presentation =
        crate::presentation::actors::entity_rig_presentation(&rig, actor, &artwork, 0.5).unwrap();
    let source = crate::actor_publication::java::hand_source(
        HandInputs {
            stream: &stream,
            presentation,
            equipment_input: &input,
            owner_equipment: &input,
            consume_ticks: None,
            item_animation: Some(actor_animation::AttachableAnimationInput {
                first_person: true,
                ..Default::default()
            }),
            alpha: 0.5,
            artwork: &artwork,
            motion: bevy::math::Mat4::IDENTITY,
            sampling_camera: None,
            aspect: 1.0,
        },
        &mut equipment,
        &mut crate::actor_publication::java::HandCache::default(),
    )
    .unwrap();
    let (draw, atlas) = source.items[1].as_ref().expect("offhand shield");
    assert_eq!(draw.presentation.submission.texture_layer, 0);
    assert_eq!(&atlas.rgba8[..3], assets::banner::color_rgb(15));
}
