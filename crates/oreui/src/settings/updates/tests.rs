use super::*;
use crate::review_tests::paint;
use launcher::menu::{
    MenuView, settings_options::SettingId, settings_sections::SettingSection, updates::Status,
};
use std::collections::HashMap;

/// Paints the selected section without clipping so tests can inspect its user-visible controls.
fn render(view: &MenuView, width: f32) -> (Vec<MenuAction>, String) {
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [0.0, width],
            column_width: width,
            y: 0.0,
            translate: &|_| None,
            nested: false,
        };
        super::super::sections::draw(
            &mut content,
            SettingSection::from_index(view.settings.section).unwrap(),
        )
        .unwrap();
    });
    let text = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
        .replace(' ', "");
    (hits.into_iter().map(|(action, _)| action).collect(), text)
}

/// Manual checks remain available with automatic checks off; busy or in-world clients cannot retry.
#[test]
fn check_now_respects_worker_progress_and_the_home_menu_boundary() {
    for width in [320.0, 800.0] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings.section = SettingSection::Updates.definition().index;
        std::sync::Arc::make_mut(&mut view.settings_options)
            .set(SettingId::AutoUpdateEnabled.index(), 0);
        for status in [
            Status::Idle,
            Status::Checking,
            Status::Downloading,
            Status::Verifying,
            Status::UpToDate,
            Status::Failed,
        ] {
            view.updates.status = status;
            let (actions, text) = render(&view, width);
            assert_eq!(
                actions.contains(&MenuAction::Update(Action::CheckNow)),
                !status.is_busy()
            );
            assert!(text.contains(&installed_version(&view.updates.running).replace(' ', "")));
            assert!(text.contains(&view.updates.status_text().replace(' ', "")));
            assert!(text.contains("Beta") && text.contains("Nightly"));
        }
        view.over_world = true;
        assert!(
            !render(&view, width)
                .0
                .contains(&MenuAction::Update(Action::CheckNow))
        );
        view.updates.active = false;
        let (actions, text) = render(&view, width);
        for setting in [SettingId::ModUpdateChecks, SettingId::ModAutoUpdate] {
            assert!(actions.contains(&MenuAction::SettingsOption(setting.index() as u16, 0)));
        }
        assert!(actions.iter().all(|action| matches!(action,
            MenuAction::SettingsOption(index, _)
                if matches!(launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(*index)].id,
                    SettingId::ModUpdateChecks | SettingId::ModAutoUpdate)
        )));
        assert!(text.contains(&installed_version(&view.updates.running).replace(' ', "")));
    }
}

/// Ready updates and older channel switches finish from the same settings page at both layouts.
#[test]
fn settings_offer_ready_install_actions_without_returning_home() {
    use launcher::menu::updates::Ready;
    use update_manifest::{build::Build, channel::Channel};
    for width in [320.0, 800.0] {
        for writable in [false, true] {
            for switching in [false, true] {
                let mut view = MenuView::new(true, "Player".into());
                view.settings.section = SettingSection::Updates.definition().index;
                let ready = Ready {
                    release: Build {
                        version: "1.0.0".into(),
                        number: 7,
                        channel: Some(Channel::Beta),
                    },
                    switching,
                    writable,
                    protocol_required: false,
                };
                let action = ready.action();
                let label = ready.action_label().replace(' ', "");
                let title = build_title(&ready.release).replace(' ', "");
                view.updates.ready = Some(ready);
                let (actions, text) = render(&view, width);
                assert!(actions.contains(&MenuAction::Update(action)));
                assert!(text.contains(&label));
                assert_eq!(text.matches(title.as_str()).count(), 1);
                assert!(actions.contains(&MenuAction::Update(Action::CheckNow)));
                view.over_world = true;
                assert!(
                    !render(&view, width)
                        .0
                        .iter()
                        .any(|action| matches!(action, MenuAction::Update(_)))
                );
            }
        }
    }
}

/// Older releases expose an explicit install or wait choice, while showing the running channel.
#[test]
fn older_channel_choices_distinguish_running_build_from_subscription() {
    use update_manifest::{build::Build, channel::Channel};
    let mut view = MenuView::new(true, "Player".into());
    view.settings.section = SettingSection::Updates.definition().index;
    view.updates.running = Build {
        version: "1.0.0".into(),
        number: 8,
        channel: Some(Channel::Nightly),
    };
    view.updates.target = Some(Build {
        version: "1.0.0".into(),
        number: 7,
        channel: Some(Channel::Beta),
    });
    view.updates.switch_available = true;
    view.updates.status = Status::UpToDate;
    for width in [320.0, 800.0] {
        let (actions, text) = render(&view, width);
        assert!(text.contains("RunningNightly"));
        assert!(text.contains("FollowingBeta"));
        assert!(text.contains("InstallBetanow") && text.contains("WaitfornextBeta"));
        assert!(!text.contains("You'reuptodate"));
        assert!(actions.contains(&MenuAction::Update(Action::SwitchNow)));
        assert!(actions.contains(&MenuAction::Update(Action::WaitForChannel)));
        view.updates.waiting_for_channel = true;
        let (actions, text) = render(&view, width);
        assert!(text.contains("WaitingforthenextnewerBeta"));
        assert!(!actions.contains(&MenuAction::Update(Action::WaitForChannel)));
        assert!(actions.contains(&MenuAction::Update(Action::SwitchNow)));
        view.updates.waiting_for_channel = false;
    }
}

/// The page shows measured byte progress with the target's channel, version and build.
#[test]
fn download_progress_identifies_the_target_release() {
    use launcher::menu::updates::DownloadProgress;
    use update_manifest::{build::Build, channel::Channel};
    let mut view = MenuView::new(true, "Player".into());
    view.settings.section = SettingSection::Updates.definition().index;
    view.updates.status = Status::Downloading;
    view.updates.target = Some(Build {
        version: "1.2.3".into(),
        number: 42,
        channel: Some(Channel::Nightly),
    });
    view.updates.download = Some(DownloadProgress {
        received_bytes: 25,
        total_bytes: 100,
    });
    for width in [320.0, 800.0] {
        let (actions, text) = render(&view, width);
        assert!(text.contains("Nightly1.2.3") && text.contains("Build42"));
        assert!(text.contains("25%"));
        assert!(!actions.contains(&MenuAction::Update(Action::CheckNow)));
    }
}

/// App and mod update controls belong to Updates and are absent from General.
#[test]
fn updates_controls_belong_to_their_own_settings_tab() {
    let mut view = MenuView::new(true, "Player".into());
    view.settings.section = SettingSection::Updates.definition().index;
    let (updates, _) = render(&view, 800.0);
    assert!(updates.contains(&MenuAction::Update(Action::CheckNow)));
    for setting in [SettingId::ModUpdateChecks, SettingId::ModAutoUpdate] {
        assert!(updates.contains(&MenuAction::SettingsOption(setting.index() as u16, 0)));
    }
    view.settings.section = SettingSection::General.definition().index;
    let (general, _) = render(&view, 800.0);
    assert!(!general.iter().any(|action| match action {
        MenuAction::Update(_) => true,
        MenuAction::SettingsOption(index, _) => matches!(
            launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(*index)].id,
            SettingId::UpdateChannel
                | SettingId::AutoUpdateEnabled
                | SettingId::InstallUpdatesOnQuit
                | SettingId::ModUpdateChecks
                | SettingId::ModAutoUpdate
        ),
        _ => false,
    }));
}
