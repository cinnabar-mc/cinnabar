//! Resource and behaviour pack handling of the worlds menu: editing a saved world and creating one.

use super::*;
use crate::local_worlds::packs::{PackFile, PackKind, StagedPack, WorldPack, WorldPacks};

fn pack(id: &str) -> WorldPack {
    WorldPack {
        id: id.to_owned(),
        version: [1, 0, 0],
        name: id.to_owned(),
        description: String::new(),
        external: false,
    }
}

/// A menu editing a BDS world whose resource list is `a`, `b`.
fn editing_packs() -> WorldsMenu {
    let mut menu = WorldsMenu::default();
    menu.apply(Event::Listed(vec![World {
        backend: Backend::Bds,
        ..world("id0", "Home")
    }]));
    assert_eq!(
        menu.update(Input::BeginEdit(0)),
        vec![Effect::LoadPacks("id0".to_owned())]
    );
    menu.apply(Event::Packs {
        id: "id0".to_owned(),
        packs: WorldPacks {
            resource: vec![pack("a"), pack("b")],
            behaviour: Vec::new(),
        },
    });
    menu
}

#[test]
fn pack_tabs_open_only_for_a_bds_world() {
    let mut menu = editing_packs();
    menu.update(Input::SelectTab(Tab::ResourcePacks));
    assert_eq!(menu.view().tab, Tab::ResourcePacks);
    assert!(!menu.view().tab_available(Tab::Experiments));

    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SelectTab(Tab::BehaviourPacks));
    assert_eq!(
        menu.view().tab,
        Tab::General,
        "Dragonfly worlds have no packs"
    );
    menu.update(Input::Back);
    menu.update(Input::BeginCreate);
    menu.update(Input::SelectTab(Tab::ResourcePacks));
    assert_eq!(
        menu.view().tab,
        Tab::ResourcePacks,
        "a BDS world shows the tab before it exists"
    );
}

#[test]
fn pack_edits_hold_the_form_until_the_worker_answers() {
    let mut menu = editing_packs();
    let effects = menu.update(Input::MovePack(PackKind::Resource, 1, true));
    assert_eq!(
        effects,
        vec![Effect::MovePack {
            id: "id0".to_owned(),
            kind: PackKind::Resource,
            pack: "b".to_owned(),
            to: 0,
        }]
    );
    assert!(menu.busy());
    assert!(
        menu.update(Input::RemovePack(PackKind::Resource, 0))
            .is_empty()
    );
    menu.apply(Event::PackFailed {
        id: "id0".to_owned(),
        message: "world is open".to_owned(),
    });
    assert!(!menu.busy());
    assert_eq!(menu.view().pack_error.as_deref(), Some("world is open"));
    assert_eq!(menu.view().packs.unwrap().resource.len(), 2);

    let effects = menu.update(Input::RemovePack(PackKind::Resource, 0));
    assert_eq!(
        effects,
        vec![Effect::RemovePack {
            id: "id0".to_owned(),
            kind: PackKind::Resource,
            pack: "a".to_owned(),
        }]
    );
    assert_eq!(
        menu.view().pack_error,
        None,
        "a new request clears the error"
    );
}

#[test]
fn pack_moves_stop_at_the_ends_and_other_worlds_answers_are_ignored() {
    let mut menu = editing_packs();
    assert!(
        menu.update(Input::MovePack(PackKind::Resource, 0, true))
            .is_empty()
    );
    assert!(
        menu.update(Input::MovePack(PackKind::Resource, 1, false))
            .is_empty()
    );
    assert!(
        menu.update(Input::RemovePack(PackKind::Resource, 9))
            .is_empty()
    );
    assert!(!menu.busy());
    menu.apply(Event::Packs {
        id: "other".to_owned(),
        packs: WorldPacks::default(),
    });
    assert_eq!(menu.view().packs.unwrap().resource.len(), 2);
    assert_eq!(
        menu.update(Input::AddPack(PackKind::Behaviour)),
        vec![Effect::AddPack {
            id: "id0".to_owned(),
            kind: PackKind::Behaviour,
        }]
    );
}

fn staged(kind: PackKind, file: &str, id: &str) -> StagedPack {
    StagedPack {
        file: PackFile {
            kind,
            path: file.into(),
        },
        pack: pack(id),
    }
}

fn ids(menu: &WorldsMenu, kind: PackKind) -> Vec<String> {
    menu.view()
        .packs
        .expect("the create tabs always list")
        .list(kind)
        .iter()
        .map(|pack| pack.id.clone())
        .collect()
}

#[test]
fn packs_chosen_while_creating_ride_with_the_new_world() {
    use PackKind::{Behaviour, Resource};
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SelectTab(Tab::ResourcePacks));
    assert_eq!(menu.view().tab, Tab::ResourcePacks);
    assert!(ids(&menu, Resource).is_empty());

    assert_eq!(
        menu.update(Input::AddPack(Resource)),
        vec![Effect::StagePack(Resource)]
    );
    assert!(menu.busy(), "the form waits for the file to be read");
    menu.apply(Event::PackStaged(None));
    assert!(!menu.busy(), "a cancelled dialog changes nothing");
    assert!(ids(&menu, Resource).is_empty());

    menu.apply(Event::PackStageFailed("Not a pack".to_owned()));
    assert_eq!(menu.view().pack_error.as_deref(), Some("Not a pack"));

    for (kind, file, id) in [
        (Resource, "a.mcpack", "a"),
        (Resource, "b.mcpack", "b"),
        (Behaviour, "c.mcpack", "c"),
    ] {
        menu.update(Input::AddPack(kind));
        menu.apply(Event::PackStaged(Some(staged(kind, file, id))));
    }
    assert!(menu.view().pack_error.is_none());
    assert_eq!(
        ids(&menu, Resource),
        ["b", "a"],
        "a new pack leads its list"
    );
    menu.apply(Event::PackStaged(Some(staged(Resource, "a2.mcpack", "a"))));
    assert_eq!(
        ids(&menu, Resource),
        ["a", "b"],
        "the same pack is replaced"
    );

    assert!(menu.update(Input::MovePack(Resource, 0, false)).is_empty());
    assert_eq!(ids(&menu, Resource), ["b", "a"]);
    assert!(menu.update(Input::MovePack(Resource, 0, true)).is_empty());
    assert!(menu.update(Input::MovePack(Resource, 1, false)).is_empty());
    assert_eq!(ids(&menu, Resource), ["b", "a"], "the ends stay put");
    assert_eq!(ids(&menu, Behaviour), ["c"]);

    menu.update(Input::SetName("Fresh".to_owned()));
    let effects = menu.update(Input::SubmitCreate);
    let [Effect::Create(new_world)] = effects.as_slice() else {
        panic!("expected one create effect, got {effects:?}");
    };
    let installs: Vec<_> = new_world
        .packs
        .iter()
        .map(|file| file.path.to_str().unwrap())
        .collect();
    assert_eq!(
        installs,
        ["a2.mcpack", "c.mcpack", "b.mcpack"],
        "installed last to first, so each list's first pack ends first"
    );
}

#[test]
fn staged_packs_are_dropped_with_the_screen() {
    use PackKind::Resource;
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginCreate);
    menu.update(Input::AddPack(Resource));
    menu.apply(Event::PackStaged(Some(staged(Resource, "a.mcpack", "a"))));
    menu.update(Input::Back);
    menu.update(Input::BeginCreate);
    assert!(ids(&menu, Resource).is_empty(), "a new world starts empty");
}

#[test]
fn only_a_pack_the_world_lacks_can_be_found_in_the_game_folders() {
    let mut menu = editing_packs();
    menu.apply(Event::Packs {
        id: "id0".to_owned(),
        packs: WorldPacks {
            resource: vec![
                pack("a"),
                WorldPack {
                    external: true,
                    ..pack("b")
                },
            ],
            behaviour: Vec::new(),
        },
    });
    assert!(
        menu.update(Input::FindPack(PackKind::Resource, 0))
            .is_empty(),
        "a pack the world holds has nothing to find"
    );
    assert!(!menu.busy());
    assert!(
        menu.update(Input::FindPack(PackKind::Resource, 9))
            .is_empty()
    );
    assert_eq!(
        menu.update(Input::FindPack(PackKind::Resource, 1)),
        vec![Effect::FindPack {
            id: "id0".to_owned(),
            kind: PackKind::Resource,
            pack: "b".to_owned(),
        }]
    );
    assert!(menu.busy(), "the form waits for the copy");
    menu.apply(Event::PackFailed {
        id: "id0".to_owned(),
        message: "No copy".to_owned(),
    });
    assert!(!menu.busy());
    assert_eq!(menu.view().pack_error.as_deref(), Some("No copy"));
}
