//! Launcher layout at the window sizes and GUI scales players use.

use update_manifest::channel::Channel;

use super::*;
use crate::screen::{Account, Server};

/// Loads the open font directly, before any installed carriers exist.
fn font() -> Text {
    Text::from_bytes(include_bytes!(
        "../../../../assets/fonts/CinnanglesSans.ttf"
    ))
    .unwrap()
}

fn overview(servers: usize) -> Overview {
    Overview {
        version: "0.1.20".into(),
        channel: Channel::Beta,
        resources_ready: false,
        data_folder: "/home/someone/.local/share/cinnabar/with/a/rather/long/path".into(),
        development: false,
        account: Some(Account {
            gamertag: "AVeryLongGamertagIndeed".into(),
            picture: None,
        }),
        servers: (0..servers)
            .map(|index| Server {
                name: format!("A server with a long name, number {index}"),
                address: format!("play{index}.example.net:19132"),
                joined: Some("Joined 2 days ago".into()),
            })
            .collect(),
    }
}

/// Every offered button gets one on-screen hit target, and nothing else does.
#[test]
fn every_page_and_launch_state_places_exactly_its_offered_buttons_on_screen() {
    let mut text = font();
    let mut art = Art::new(bootstrap_ui::decode_logo(), None);
    for servers in [0, 2, 9] {
        let overview = overview(servers);
        let mut screens: Vec<_> = Page::ALL
            .into_iter()
            .map(|page| Screen::at(page, Launch::Idle, servers))
            .collect();
        screens.push(Screen::at(Page::Home, Launch::Starting, servers));
        screens.push(Screen::at(
            Page::Servers,
            Launch::Failed {
                message: "word ".repeat(400),
            },
            servers,
        ));
        for size in [[560, 420], [1200, 720], [2560, 1440]] {
            for offset in [-3, 0] {
                for screen in &screens {
                    let mut canvas = Canvas::new(size[0], size[1]);
                    let style = Style {
                        rem: f32::from(
                            ui::DesktopGuiScale::for_window(size).scale_for_offset(offset),
                        ) * oreui::GUI_PIXELS_PER_REM,
                        hovered: None,
                        focused: None,
                        pressed: None,
                    };
                    let hits = draw(&mut canvas, &mut text, &mut art, screen, &overview, &style);
                    let mut placed: Vec<_> = hits.iter().map(|(action, _)| *action).collect();
                    let mut offered = screen.actions().to_vec();
                    let key = |action: &Action| format!("{action:?}");
                    placed.sort_by_key(key);
                    offered.sort_by_key(key);
                    assert_eq!(placed, offered, "{size:?} {screen:?}");
                    for (action, rect) in hits {
                        assert!(
                            rect.x >= 0.0
                                && rect.y >= 0.0
                                && rect.x + rect.w <= size[0] as f32 + 0.5
                                && rect.y + rect.h <= size[1] as f32 + 0.5,
                            "{action:?} at {rect:?} leaves {size:?}"
                        );
                    }
                }
            }
        }
    }
}
