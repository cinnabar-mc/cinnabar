//! Asset-independent drawing of the launcher window: a sidebar, the current page over the
//! panorama, a bottom bar with the account and Play, and the failure modal.

mod pages;

use bootstrap_ui::canvas::{Canvas, Image, Rect, Text};
use image::imageops::{self, FilterType};
use ui::oreui_theme::{self as oreui, Rgba, Type};

use crate::{
    screen::{Action, Launch, Overview, Page, Screen},
    theme,
};

/// The sidebar's width in rem.
const SIDEBAR_WIDTH: f32 = 22.0;
/// The bottom bar's height in rem.
const BAR_HEIGHT: f32 = 9.6;
/// The narrowest and shortest window, in rem, that keeps the design scale; smaller windows
/// shrink the rem instead.
const DESIGN_SIZE: [f32; 2] = [100.0, 62.0];
/// Lines of a failure message beyond this are cut so the modal's buttons stay on screen.
const MAX_MESSAGE_LINES: usize = 8;

/// How one frame draws: scale and the interaction state of its buttons.
pub struct Style {
    /// Physical pixels per rem, from the same GUI-scale authority as the menus.
    pub rem: f32,
    pub hovered: Option<Action>,
    pub focused: Option<Action>,
    pub pressed: Option<Action>,
}

/// The launcher's images, resampled once per drawn size instead of every frame.
#[derive(Default)]
pub struct Art {
    logo: Option<Image>,
    avatar: Option<Image>,
    scaled: Vec<(bool, u32, u32, Image)>,
}

impl Art {
    /// The product logo and the account picture, either of which may be missing.
    pub fn new(logo: Option<Image>, avatar: Option<Image>) -> Self {
        Self {
            logo,
            avatar,
            scaled: Vec::new(),
        }
    }

    /// Decodes the account picture at `path`; a missing or undecodable file gives `None`.
    pub fn load_avatar(path: &str) -> Option<Image> {
        let rgba = image::open(path).ok()?.into_rgba8();
        Some(Image {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        })
    }

    /// The logo's height at `width` pixels wide.
    fn logo_height(&self, width: f32) -> f32 {
        self.logo.as_ref().map_or(0.0, |logo| {
            width * logo.height as f32 / logo.width.max(1) as f32
        })
    }

    /// The logo (or, with `avatar`, the account picture) smoothly resampled to `rect`'s size.
    fn sized(&mut self, avatar: bool, rect: Rect) -> Option<&Image> {
        let (w, h) = (
            rect.w.round().max(1.0) as u32,
            rect.h.round().max(1.0) as u32,
        );
        let source = if avatar { &self.avatar } else { &self.logo };
        let source = source.as_ref()?;
        let found = self
            .scaled
            .iter()
            .position(|(a, sw, sh, _)| *a == avatar && *sw == w && *sh == h);
        let index = match found {
            Some(index) => index,
            None => {
                let buffer =
                    image::RgbaImage::from_raw(source.width, source.height, source.rgba.clone())?;
                let resized = imageops::resize(&buffer, w, h, FilterType::Triangle);
                // Only the current window size matters; drop sizes from earlier resizes.
                self.scaled.retain(|(a, ..)| *a != avatar);
                self.scaled.push((
                    avatar,
                    w,
                    h,
                    Image {
                        width: w,
                        height: h,
                        rgba: resized.into_raw(),
                    },
                ));
                self.scaled.len() - 1
            }
        };
        Some(&self.scaled[index].3)
    }
}

/// One frame's drawing state: the canvas, scale, input highlights and collected hit targets.
struct Painter<'a> {
    canvas: &'a mut Canvas,
    text: &'a mut Text,
    art: &'a mut Art,
    screen: &'a Screen,
    style: &'a Style,
    rem: f32,
    /// Set while drawing under the failure modal, whose buttons alone take input.
    blocked: bool,
    hits: Vec<(Action, Rect)>,
}

impl Painter<'_> {
    /// Whether `action`'s button takes input in this part of the frame.
    fn offered(&self, action: Action) -> bool {
        !self.blocked && self.screen.actions().contains(&action)
    }

    /// `ty`'s font size in pixels.
    fn px(&self, ty: Type) -> f32 {
        ty.size * self.rem
    }

    /// Draws one line of `ty` text with its top at `y`.
    fn label(&mut self, x: f32, y: f32, ty: Type, color: Rgba, label: &str) {
        let px = self.px(ty);
        self.text.draw(self.canvas, x, y, px, color, label);
    }

    /// Draws one line of `ty` text vertically centred in `rect`, starting at `x`.
    fn label_in(&mut self, rect: Rect, x: f32, ty: Type, color: Rgba, label: &str) {
        let px = self.px(ty);
        let y = rect.y + (rect.h - self.text.line_height(px)) * 0.5;
        self.text.draw(self.canvas, x, y, px, color, label);
    }

    /// `label` cut with an ellipsis to fit `width` pixels at `ty`.
    fn fit(&mut self, label: &str, ty: Type, width: f32) -> String {
        let px = self.px(ty);
        self.text.truncate_end(label, px, width.max(0.0))
    }

    fn width(&mut self, label: &str, ty: Type) -> f32 {
        let px = self.px(ty);
        self.text.width(label, px)
    }

    fn line(&self, ty: Type) -> f32 {
        (ty.line * self.rem).max(self.text.line_height(self.px(ty)))
    }

    fn hovered(&self, action: Action) -> bool {
        self.style.hovered == Some(action) && self.offered(action)
    }

    fn pressed(&self, action: Action) -> bool {
        self.style.pressed == Some(action)
    }

    /// Records `rect` as `action`'s hit target, if the screen currently offers it.
    fn hit(&mut self, action: Action, rect: Rect) {
        if self.offered(action) {
            self.hits.push((action, rect));
        }
    }

    /// Outlines `rect` when `action` has keyboard or controller focus.
    fn focus(&mut self, action: Action, rect: Rect) {
        if self.style.focused == Some(action) && self.offered(action) {
            let edge = oreui::EDGE * self.rem;
            self.canvas
                .frame(rect.inset(-edge * 1.5), edge, theme::FOCUS);
        }
    }

    /// Fills `rect` with a left-to-right fade from `from` to `to`.
    fn fade(&mut self, rect: Rect, from: Rgba, to: Rgba) {
        const STEPS: usize = 48;
        let step = rect.w / STEPS as f32;
        for index in 0..STEPS {
            let t = index as f32 / (STEPS - 1) as f32;
            let color = std::array::from_fn(|channel| {
                (f32::from(from[channel]) * (1.0 - t) + f32::from(to[channel]) * t).round() as u8
            });
            // Whole-pixel edges, so neighbouring strips neither overlap nor leave gaps.
            let left = (rect.x + step * index as f32).round();
            let right = (rect.x + step * (index + 1) as f32).round();
            self.canvas.fill(
                Rect {
                    x: left,
                    w: right - left,
                    ..rect
                },
                color,
            );
        }
    }

    /// A raised square button in the accent (`primary`) or a neutral surface, labelled in
    /// `ty`, registered as `action`'s hit target.
    fn button(&mut self, action: Action, rect: Rect, primary: bool, ty: Type, label: &str) {
        let enabled = self.offered(action);
        let (hovered, pressed) = (
            enabled && self.hovered(action),
            enabled && self.pressed(action),
        );
        let depth = oreui::BUTTON_DEPTH * self.rem * 1.5;
        let face = Rect {
            y: rect.y + if pressed { depth } else { 0.0 },
            h: rect.h - depth,
            ..rect
        };
        let (fill, shadow) = match (primary, enabled) {
            (true, true) if pressed => (theme::ACCENT_PRESSED, theme::ACCENT_SHADOW),
            (true, true) if hovered => (theme::ACCENT_HOVER, theme::ACCENT_SHADOW),
            (true, true) => (theme::ACCENT, theme::ACCENT_SHADOW),
            (true, false) => (theme::ACCENT_PRESSED, theme::ACCENT_SHADOW),
            (false, _) if hovered => (theme::SURFACE_HOVER, theme::SIDEBAR),
            (false, _) => (theme::SURFACE_RAISED, theme::SIDEBAR),
        };
        if !pressed {
            self.canvas.fill(
                Rect {
                    y: face.y + face.h,
                    h: depth,
                    ..face
                },
                shadow,
            );
        }
        self.canvas.fill(face, fill);
        let edge = oreui::EDGE * self.rem;
        self.canvas
            .fill(Rect { h: edge, ..face }, theme::ACCENT_SPECULAR);
        let width = self.width(label, ty);
        let ink = if primary || enabled {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        self.label_in(face, face.x + (face.w - width) * 0.5, ty, ink, label);
        self.hit(action, rect);
        self.focus(action, rect);
    }
}

/// Draws one launcher frame and returns the hit target of every button it currently offers.
pub fn draw(
    canvas: &mut Canvas,
    text: &mut Text,
    art: &mut Art,
    screen: &Screen,
    overview: &Overview,
    style: &Style,
) -> Vec<(Action, Rect)> {
    let (width, height) = (canvas.width as f32, canvas.height as f32);
    let rem = style
        .rem
        .min(width / DESIGN_SIZE[0])
        .min(height / DESIGN_SIZE[1])
        .max(1.0);
    let sidebar = Rect {
        x: 0.0,
        y: 0.0,
        w: SIDEBAR_WIDTH * rem,
        h: height,
    };
    let bar = Rect {
        x: sidebar.w,
        y: height - BAR_HEIGHT * rem,
        w: width - sidebar.w,
        h: BAR_HEIGHT * rem,
    };
    let main = Rect {
        x: sidebar.w,
        y: 0.0,
        w: bar.w,
        h: bar.y,
    };
    let mut painter = Painter {
        canvas,
        text,
        art,
        screen,
        style,
        rem,
        blocked: matches!(screen.launch, Launch::Failed { .. }),
        hits: Vec::new(),
    };
    painter.fade(
        main,
        theme::with_alpha(theme::SIDEBAR, 0xe0),
        theme::with_alpha(theme::SIDEBAR, 0x50),
    );
    pages::draw(&mut painter, main, overview);
    draw_sidebar(&mut painter, sidebar);
    draw_bar(&mut painter, bar, overview);
    if let Launch::Failed { message } = &screen.launch {
        painter.blocked = false;
        draw_modal(&mut painter, message);
    }
    painter.hits
}

/// The logo and page navigation.
fn draw_sidebar(painter: &mut Painter<'_>, sidebar: Rect) {
    let rem = painter.rem;
    painter.canvas.fill(sidebar, theme::SIDEBAR);
    painter.canvas.fill(
        Rect {
            x: sidebar.x + sidebar.w - oreui::EDGE * rem,
            w: oreui::EDGE * rem,
            ..sidebar
        },
        theme::DIVIDER,
    );
    let pad = 2.4 * rem;
    let logo_w = sidebar.w - pad * 2.0;
    let logo = Rect {
        x: pad,
        y: 3.2 * rem,
        w: logo_w,
        h: painter.art.logo_height(logo_w),
    };
    if let Some(image) = painter.art.sized(false, logo) {
        painter.canvas.image(image, logo);
    }
    let mut y = logo.y + logo.h + 4.0 * rem;
    let item_h = 4.8 * rem;
    for page in Page::ALL {
        let action = Action::Open(page);
        let item = Rect {
            x: 1.2 * rem,
            y,
            w: sidebar.w - 2.4 * rem,
            h: item_h,
        };
        let active = painter.screen.page == page;
        if active {
            painter.canvas.fill(item, theme::SURFACE_RAISED);
            painter.canvas.fill(
                Rect {
                    w: 0.4 * rem,
                    ..item
                },
                theme::ACCENT,
            );
        } else if painter.hovered(action) {
            painter.canvas.fill(item, theme::SURFACE);
        }
        let ink = if active || painter.hovered(action) {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        painter.label_in(
            item,
            item.x + 2.0 * rem,
            oreui::SECTION_HEADER,
            ink,
            page.label(),
        );
        painter.hit(action, item);
        painter.focus(action, item);
        y += item_h + 0.4 * rem;
    }
}

/// The account on the left and the readiness line and Play button on the right.
fn draw_bar(painter: &mut Painter<'_>, bar: Rect, overview: &Overview) {
    let rem = painter.rem;
    painter.canvas.fill(bar, theme::BAR);
    painter.canvas.fill(
        Rect {
            h: oreui::EDGE * rem,
            ..bar
        },
        theme::DIVIDER,
    );
    let pad = 3.2 * rem;
    let play = Rect {
        x: bar.x + bar.w - pad - 24.0 * rem,
        y: bar.y + (bar.h - 6.0 * rem) * 0.5,
        w: 24.0 * rem,
        h: 6.0 * rem,
    };
    let starting = painter.screen.launch == Launch::Starting;
    let big = Type {
        size: 2.4,
        ..oreui::PRIMARY_BUTTON
    };
    painter.button(
        Action::Play,
        play,
        true,
        big,
        if starting { "STARTING…" } else { "PLAY" },
    );

    let avatar = Rect {
        x: bar.x + pad,
        y: bar.y + (bar.h - 5.6 * rem) * 0.5,
        w: 5.6 * rem,
        h: 5.6 * rem,
    };
    let (name, detail) = match &overview.account {
        Some(account) => (account.gamertag.as_str(), "Xbox account"),
        None => ("Not signed in", "Sign in from the game's menu"),
    };
    painter
        .canvas
        .fill(avatar.inset(-oreui::EDGE * rem), theme::BORDER);
    match painter.art.sized(true, avatar) {
        Some(image) => painter.canvas.image(image, avatar),
        None => {
            let initial = name
                .chars()
                .next()
                .filter(|_| overview.account.is_some())
                .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
            painter.canvas.fill(
                avatar,
                if overview.account.is_some() {
                    theme::ACCENT_PRESSED
                } else {
                    theme::SURFACE_RAISED
                },
            );
            let width = painter.width(&initial, oreui::HEADER5);
            painter.label_in(
                avatar,
                avatar.x + (avatar.w - width) * 0.5,
                oreui::HEADER5,
                theme::TEXT,
                &initial,
            );
        }
    }

    let status_right = play.x - 2.4 * rem;
    let (status, color) = if overview.resources_ready {
        ("Ready to play", theme::READY)
    } else {
        ("Resources download on first play", theme::PENDING)
    };
    let detail_line = format!("v{} ({})", overview.version, overview.channel_label());
    let status_w = painter.width(status, oreui::BODY);
    let detail_w = painter.width(&detail_line, oreui::CAPTION);
    let body = painter.line(oreui::BODY);
    let caption = painter.line(oreui::CAPTION);
    let top = bar.y + (bar.h - body - caption) * 0.5;
    let dot = 0.8 * rem;
    let status_x = status_right - status_w;
    painter.canvas.fill(
        Rect {
            x: status_x - dot - 0.8 * rem,
            y: top + (body - dot) * 0.5,
            w: dot,
            h: dot,
        },
        color,
    );
    painter.label(status_x, top, oreui::BODY, theme::TEXT, status);
    painter.label(
        status_right - detail_w,
        top + body,
        oreui::CAPTION,
        theme::TEXT_FAINT,
        &detail_line,
    );

    let text_x = avatar.x + avatar.w + 1.6 * rem;
    let room = status_x - dot - 3.2 * rem - text_x;
    let name = painter.fit(name, oreui::BODY, room);
    let detail = painter.fit(detail, oreui::CAPTION, room);
    painter.label(text_x, top, oreui::BODY, theme::TEXT, &name);
    painter.label(
        text_x,
        top + body,
        oreui::CAPTION,
        theme::TEXT_FAINT,
        &detail,
    );
}

/// Dims the launcher and explains why the client did not start, offering another try.
fn draw_modal(painter: &mut Painter<'_>, message: &str) {
    let rem = painter.rem;
    let (width, height) = (painter.canvas.width as f32, painter.canvas.height as f32);
    painter.canvas.fill(
        Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        },
        theme::SCRIM,
    );
    let pad = 3.2 * rem;
    let panel_w = (56.0 * rem).min(width - 4.0 * rem);
    let inner_w = panel_w - pad * 2.0;
    let body_px = painter.px(oreui::BODY);
    let mut lines = painter.text.wrap(message, body_px, inner_w);
    if lines.len() > MAX_MESSAGE_LINES {
        lines.truncate(MAX_MESSAGE_LINES);
        lines.push("…".into());
    }
    let title = painter.line(oreui::HEADER5);
    let body = painter.line(oreui::BODY);
    let button_h = 4.8 * rem;
    let panel_h = pad * 2.0 + title + 1.6 * rem + body * lines.len() as f32 + 3.2 * rem + button_h;
    let panel = Rect {
        x: (width - panel_w) * 0.5,
        y: ((height - panel_h) * 0.5).max(0.0),
        w: panel_w,
        h: panel_h,
    };
    painter.canvas.fill(
        Rect {
            x: panel.x + 0.8 * rem,
            y: panel.y + 0.8 * rem,
            ..panel
        },
        oreui::TEXT_SHADOW,
    );
    painter
        .canvas
        .fill(panel, theme::with_alpha(theme::SURFACE, 0xff));
    painter.canvas.fill(
        Rect {
            h: 0.4 * rem,
            ..panel
        },
        theme::ACCENT,
    );
    let x = panel.x + pad;
    let mut y = panel.y + pad;
    painter.label(
        x,
        y,
        oreui::HEADER5,
        theme::TEXT,
        concat!("Couldn't start ", launcher::product_name!()),
    );
    y += title + 1.6 * rem;
    for line in &lines {
        painter.label(x, y, oreui::BODY, theme::TEXT_DIM, line);
        y += body;
    }
    y += 3.2 * rem;
    let button_w = 16.0 * rem;
    let retry = Rect {
        x: panel.x + panel.w - pad - button_w,
        y,
        w: button_w,
        h: button_h,
    };
    let close = Rect {
        x: retry.x - 1.2 * rem - button_w,
        ..retry
    };
    painter.button(
        Action::Dismiss,
        close,
        false,
        oreui::SECONDARY_BUTTON,
        "Close",
    );
    painter.button(
        Action::Play,
        retry,
        true,
        oreui::SECONDARY_BUTTON,
        "Try again",
    );
}

#[cfg(test)]
mod tests;
