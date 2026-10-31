//! Window-free PNG captures of the launcher's screens over the built-in panorama, drawn by the
//! same painters as the window, for reviewing changes without a display or GPU.

use std::path::Path;

use anyhow::{Context, Result};
use bootstrap_ui::{PANORAMA_TINT, Preferences, canvas::Canvas};
use launcher::install_layout::InstallLayout;
use render_model::panorama::{
    PanoramaFaces, PanoramaView,
    cpu::{background, linear_to_srgb, srgb_to_linear},
    launcher_view,
};
use update_manifest::channel::Channel;

use crate::{
    screen::{Account, Launch, Overview, Page, Screen, Server},
    view,
};

/// The panorama's time in seconds for every capture, so captures of one state match.
const CAPTURE_SECONDS: f32 = 12.0;

/// A launcher state a capture can show, with sample installation facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// A signed-in player with prepared resources and recent servers.
    Home,
    /// A fresh install: signed out, no saved servers, resources not yet prepared.
    FirstPlay,
    Servers,
    Installation,
    /// Play was pressed and the client is starting.
    Starting,
    /// The client executable is missing.
    Failed,
}

impl State {
    pub const ALL: [Self; 6] = [
        Self::Home,
        Self::FirstPlay,
        Self::Servers,
        Self::Installation,
        Self::Starting,
        Self::Failed,
    ];
    /// Command-line names, in [`State::ALL`] order.
    pub const NAMES: [&str; 6] = [
        "home",
        "first-play",
        "servers",
        "installation",
        "starting",
        "failed",
    ];

    /// The state named `name` on the command line.
    pub fn parse(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .map(|index| Self::ALL[index])
    }

    /// The screen and the sample installation facts it shows.
    pub fn sample(self) -> (Screen, Overview) {
        let fresh = self == Self::FirstPlay;
        let servers = if fresh {
            Vec::new()
        } else {
            SAMPLE_SERVERS
                .iter()
                .map(|(name, host, joined)| Server {
                    name: (*name).to_owned(),
                    address: format!("{host}.example.net:19132"),
                    joined: joined.map(str::to_owned),
                })
                .collect()
        };
        let overview = Overview {
            version: launcher::PRODUCT_VERSION.to_owned(),
            channel: Channel::Beta,
            resources_ready: !fresh,
            data_folder: "/home/player/.local/share/cinnabar".to_owned(),
            development: false,
            account: (!fresh).then(|| Account {
                gamertag: "PlayerOne".to_owned(),
                picture: None,
            }),
            servers,
        };
        let count = overview.servers.len();
        let screen = match self {
            Self::Home | Self::FirstPlay => Screen::new(count),
            Self::Servers => Screen::at(Page::Servers, Launch::Idle, count),
            Self::Installation => Screen::at(Page::Installation, Launch::Idle, count),
            Self::Starting => Screen::at(Page::Home, Launch::Starting, count),
            Self::Failed => Screen::at(
                Page::Home,
                Launch::Failed {
                    message: format!(
                        "The game was not found at /opt/cinnabar/bin/cinnabar. Reinstall {} to \
                         restore it.",
                        launcher::PRODUCT_NAME
                    ),
                },
                count,
            ),
        };
        (screen, overview)
    }
}

/// Sample saved servers: name, host label under `example.net` and when each was last joined.
const SAMPLE_SERVERS: [(&str, &str, Option<&str>); 8] = [
    ("Survival Island", "survival", Some("Joined 2 hours ago")),
    ("Creative Plots", "creative", Some("Joined 1 day ago")),
    ("Skyblock", "skyblock", Some("Joined 3 days ago")),
    ("Bedwars Practice", "bedwars", Some("Joined 1 week ago")),
    ("Friends' Realm", "realm", Some("Joined 2 weeks ago")),
    ("Minigames Hub", "hub", Some("Joined 1 month ago")),
    ("Parkour Trials", "parkour", Some("Joined 3 months ago")),
    ("Test Server", "test", None),
];

/// Renders `state` at `size` physical pixels with the default menu preferences and writes it to
/// `out` as a PNG. The font comes from this executable's installation or checkout.
pub fn write(state: State, size: [u32; 2], out: &Path) -> Result<()> {
    let layout = InstallLayout::discover().context("resolve the installation for its UI font")?;
    let mut text = bootstrap_ui::text(&layout)?;
    let faces = bootstrap_ui::built_in_faces().context("decode the built-in panorama")?;
    let mut art = view::Art::new(bootstrap_ui::decode_logo(), None);
    let preferences = Preferences::default();
    let (screen, overview) = state.sample();
    let mut canvas = Canvas::new(size[0], size[1]);
    view::draw(
        &mut canvas,
        &mut text,
        &mut art,
        &screen,
        &overview,
        &view::Style {
            rem: preferences.rem(winit::dpi::PhysicalSize::new(size[0], size[1])),
            hovered: None,
            focused: None,
            pressed: None,
        },
    );
    let view = launcher_view(
        CAPTURE_SECONDS,
        size[0] as f32 / size[1] as f32,
        PANORAMA_TINT,
    );
    composite(&mut canvas.pixels, size, &faces, &view);
    image::save_buffer(
        out,
        &canvas.pixels,
        size[0],
        size[1],
        image::ColorType::Rgba8,
    )
    .with_context(|| format!("write {}", out.display()))
}

/// Composites the premultiplied overlay over the panorama in linear light, as the window's
/// premultiplied blend into its sRGB surface does, leaving every pixel opaque.
fn composite(pixels: &mut [u8], size: [u32; 2], faces: &PanoramaFaces, view: &PanoramaView) {
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let alpha = f32::from(pixel[3]) / 255.0;
        if alpha < 1.0 {
            let index = index as u32;
            let under = background(faces, view, size, index % size[0], index / size[0]);
            for channel in 0..3 {
                let over = srgb_to_linear(pixel[channel]);
                pixel[channel] =
                    linear_to_srgb(over + under[channel] * (1.0 - alpha)).round() as u8;
            }
        }
        pixel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_command_line_name() {
        for (state, name) in State::ALL.into_iter().zip(State::NAMES) {
            assert_eq!(State::parse(name), Some(state));
        }
        assert_eq!(State::parse("settings"), None);
    }

    #[test]
    fn transparent_pixels_show_the_panorama_and_opaque_ones_keep_the_overlay() {
        let faces =
            PanoramaFaces::new(1, std::array::from_fn(|_| vec![120, 160, 200, 255])).unwrap();
        let mut pixels = [0, 0, 0, 0, 12, 34, 56, 255];
        composite(
            &mut pixels,
            [2, 1],
            &faces,
            &launcher_view(0.0, 2.0, [0.0; 4]),
        );
        assert_eq!(pixels, [120, 160, 200, 255, 12, 34, 56, 255]);
    }
}
