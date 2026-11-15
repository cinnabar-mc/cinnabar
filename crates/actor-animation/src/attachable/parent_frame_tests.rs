use super::*;

fn skeleton(specs: &[(&str, Option<usize>, bool)]) -> render::LayerSkeleton {
    let mut bones: Vec<_> = specs
        .iter()
        .map(|(_, parent, binding)| RuntimeBone {
            parent: *parent,
            has_binding_expression: *binding,
            ..Default::default()
        })
        .collect();
    let names: Vec<Box<str>> = specs.iter().map(|(name, _, _)| Box::from(*name)).collect();
    bind_roots(
        &mut bones,
        &names,
        crate::attachment::OwnerSkeleton::body(&[], &[], &["rightitem".into(), "head".into()]),
    );
    render::LayerSkeleton {
        bones,
        names,
        channels: Default::default(),
    }
}

#[test]
fn selected_geometry_descendants_inherit_their_own_root_parent_frame() {
    let primary = skeleton(&[
        ("unbound", None, false),
        ("child", Some(0), false),
        ("rightitem", None, false),
        ("held_child", Some(2), false),
        ("shield", None, true),
        ("shield_child", Some(4), false),
    ]);
    let layers = BTreeMap::from([(
        1,
        Some(Arc::new(skeleton(&[
            ("head", None, false),
            ("ornament", Some(0), false),
        ]))),
    )]);
    let snapshot = AttachableRigSnapshot {
        geometry: 0,
        pose: &[],
        bone_names: &primary.names,
        render: &[],
        scale: 1.0,
        axis_scale: [1.0; 3],
        bones: &primary.bones,
        layer_skeletons: &layers,
    };
    for index in 0..2 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::Actor)
        );
        assert_eq!(
            snapshot.bone_parent(1, index),
            Some(AttachableBoneParent::OwnerNamed("head")),
        );
    }
    for index in 2..4 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::OwnerNamed("rightitem")),
        );
    }
    for index in 4..6 {
        assert_eq!(
            snapshot.bone_parent(0, index),
            Some(AttachableBoneParent::BindingExpression),
        );
    }
    assert_eq!(snapshot.bone_parent(0, 6), None);
    assert_eq!(snapshot.bone_parent(2, 0), None);
}

/// Worn armour hangs every limb under one root and has no pose of its own.
#[test]
fn owner_named_descendants_follow_their_own_owner_bone() {
    let specs = [
        ("waist", None, [0.0, 12.0, 0.0]),
        ("body", Some(0), [0.0, 24.0, 0.0]),
        ("jacket", Some(1), [0.0, 24.0, 0.0]),
        ("leftArm", Some(1), [5.0, 22.0, 0.0]),
        ("leftSleeve", Some(3), [5.0, 20.0, 0.0]),
    ];
    let mut bones: Vec<_> = specs
        .iter()
        .map(|(_, parent, pivot)| RuntimeBone {
            parent: *parent,
            pivot: *pivot,
            ..Default::default()
        })
        .collect();
    let names: Vec<Box<str>> = specs.iter().map(|(name, ..)| Box::from(*name)).collect();
    let owner: Vec<Box<str>> = ["waist", "body", "leftarm"].map(Box::from).into();
    bind_roots(
        &mut bones,
        &names,
        crate::attachment::OwnerSkeleton::body(&[], &[], &owner),
    );
    let layers = BTreeMap::new();
    let snapshot = AttachableRigSnapshot {
        geometry: 0,
        pose: &[],
        bone_names: &names,
        render: &[],
        scale: 1.0,
        axis_scale: [1.0; 3],
        bones: &bones,
        layer_skeletons: &layers,
    };
    let followed: Vec<_> = (0..bones.len())
        .map(|index| snapshot.bone_parent(0, index).unwrap())
        .collect();
    assert_eq!(
        followed,
        ["waist", "body", "body", "leftArm", "leftArm"].map(AttachableBoneParent::OwnerNamed)
    );

    // The armour's own body turn stays with the body: the arm takes its frame from the owner.
    let mut local = vec![crate::pose::LocalDelta::default(); bones.len()];
    local[1].rotation = [0.0, 0.0, 90.0];
    let pose = crate::pose::compose_pose_with_targets(&bones, &local, &[]).unwrap();
    let (waist, body, jacket, arm, sleeve) = (pose[0], pose[1], pose[2], pose[3], pose[4]);
    assert_ne!(body.rotation, waist.rotation);
    assert_eq!(jacket.rotation, body.rotation);
    for bone in [arm, sleeve] {
        assert_eq!(bone.rotation, waist.rotation);
    }
    assert_eq!(arm.translation_scale, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(sleeve.translation_scale, [0.0, -2.0, 0.0, 1.0]);
}
