//! The page area beside the sidebar: home, saved servers and installation details.

use bootstrap_ui::canvas::Rect;
use ui::oreui_theme::{self as oreui, Type};

use super::Painter;
use crate::{
    screen::{Action, LISTED_SERVERS, Overview, Page, RESOURCES_ROW, Server},
    theme,
};

/// The page heading's type: the menus' header face at a hero size.
const HERO: Type = Type {
    size: 4.0,
    line: 4.8,
    ..oreui::HEADER3
};

/// Draws the current page into `main`.
pub(super) fn draw(painter: &mut Painter<'_>, main: Rect, overview: &Overview) {
    let pad = 4.8 * painter.rem;
    let content = Rect {
        x: main.x + pad,
        y: main.y + pad,
        w: main.w - pad * 2.0,
        h: main.h - pad * 2.0,
    };
    match painter.screen.page {
        Page::Home => home(painter, content, overview),
        Page::Servers => servers(painter, content, overview),
        Page::Installation => installation(painter, content, overview),
    }
}

/// Draws an overline, heading and subtitle at the top of `content`; returns the bottom.
fn heading(
    painter: &mut Painter<'_>,
    content: Rect,
    overline: &str,
    title: &str,
    subtitle: &str,
) -> f32 {
    let mut y = content.y;
    let overline = painter.fit(overline, oreui::CAPTION, content.w);
    painter.label(content.x, y, oreui::CAPTION, theme::ACCENT, &overline);
    y += painter.line(oreui::CAPTION) + 0.4 * painter.rem;
    let title = painter.fit(title, HERO, content.w);
    painter.label(content.x, y, HERO, theme::TEXT, &title);
    y += painter.line(HERO) + 0.8 * painter.rem;
    let px = painter.px(oreui::BODY);
    let lines = painter
        .text
        .wrap(subtitle, px, content.w.min(64.0 * painter.rem));
    for line in lines {
        painter.label(content.x, y, oreui::BODY, theme::TEXT_DIM, &line);
        y += painter.line(oreui::BODY);
    }
    y
}

/// A welcome for the signed-in player and cards for the most recently joined servers.
fn home(painter: &mut Painter<'_>, content: Rect, overview: &Overview) {
    let title = match &overview.account {
        Some(account) => format!("Welcome back, {}", account.gamertag),
        None => format!("Welcome to {}", launcher::PRODUCT_NAME),
    };
    let subtitle = if overview.resources_ready {
        "Everything is ready. Press Play to open the game, or jump straight back into a server."
    } else {
        "The first time you play, the game downloads and prepares its resources. That takes a few \
         minutes and only happens once."
    };
    heading(painter, content, "HOME", &title, subtitle);

    let rem = painter.rem;
    let card_h = 10.4 * rem;
    let cards = Rect {
        y: content.y + content.h - card_h,
        h: card_h,
        ..content
    };
    painter.label(
        content.x,
        cards.y - painter.line(oreui::CAPTION) - 1.2 * rem,
        oreui::CAPTION,
        theme::TEXT_FAINT,
        "JUMP BACK IN",
    );
    let recent = overview.recent();
    if recent.is_empty() {
        empty(painter, cards);
        return;
    }
    let gap = 1.6 * rem;
    let slots = crate::screen::RECENT_SERVERS as f32;
    let card_w = (cards.w - gap * (slots - 1.0)) / slots;
    for (index, server) in recent.iter().enumerate() {
        let card = Rect {
            x: cards.x + (card_w + gap) * index as f32,
            w: card_w,
            ..cards
        };
        server_card(painter, card, index, server);
    }
}

/// The card shown instead of recent servers before any were saved.
fn empty(painter: &mut Painter<'_>, card: Rect) {
    let rem = painter.rem;
    painter.canvas.fill(card, theme::SURFACE);
    let inner = card.inset(2.4 * rem);
    painter.label(
        inner.x,
        inner.y,
        oreui::SECTION_HEADER,
        theme::TEXT,
        "No saved servers yet",
    );
    let px = painter.px(oreui::BODY);
    let mut y = inner.y + painter.line(oreui::SECTION_HEADER) + 0.8 * rem;
    let lines = painter.text.wrap(
        "Add servers from the game's Servers menu and they show up here to join in one click.",
        px,
        inner.w,
    );
    for line in lines {
        painter.label(inner.x, y, oreui::BODY, theme::TEXT_DIM, &line);
        y += painter.line(oreui::BODY);
    }
}

/// A clickable card joining `server`, the `index`th saved server.
fn server_card(painter: &mut Painter<'_>, card: Rect, index: usize, server: &Server) {
    let rem = painter.rem;
    let action = Action::Join(index);
    let lit = painter.hovered(action) || painter.style.focused == Some(action);
    painter.canvas.fill(
        card,
        if lit {
            theme::SURFACE_HOVER
        } else {
            theme::SURFACE
        },
    );
    painter.canvas.fill(
        Rect {
            w: 0.4 * rem,
            ..card
        },
        if lit { theme::ACCENT } else { theme::BORDER },
    );
    let inner = Rect {
        x: card.x + 2.4 * rem,
        w: card.w - 4.0 * rem,
        ..card.inset(2.0 * rem)
    };
    let join_w = painter.width("JOIN", oreui::CAPTION);
    let name = painter.fit(&server.name, oreui::SECTION_HEADER, inner.w - join_w - rem);
    painter.label(inner.x, inner.y, oreui::SECTION_HEADER, theme::TEXT, &name);
    painter.label(
        inner.x + inner.w - join_w,
        inner.y,
        oreui::CAPTION,
        if lit {
            theme::ACCENT_HOVER
        } else {
            theme::ACCENT
        },
        "JOIN",
    );
    let address = painter.fit(&server.address, oreui::CAPTION, inner.w);
    let y = inner.y + painter.line(oreui::SECTION_HEADER) + 0.2 * rem;
    painter.label(inner.x, y, oreui::CAPTION, theme::TEXT_FAINT, &address);
    let joined = server.joined.as_deref().unwrap_or("Never joined");
    let joined = painter.fit(joined, oreui::CAPTION, inner.w);
    painter.label(
        inner.x,
        inner.y + inner.h - painter.line(oreui::CAPTION),
        oreui::CAPTION,
        theme::TEXT_DIM,
        &joined,
    );
    painter.hit(action, card);
    painter.focus(action, card);
}

/// Every saved server, up to [`LISTED_SERVERS`], as rows that join it.
fn servers(painter: &mut Painter<'_>, content: Rect, overview: &Overview) {
    let count = overview.servers.len();
    let subtitle = match count {
        0 => "Servers you save in the game appear here.".to_owned(),
        1 => "1 saved server. Select it to join directly.".to_owned(),
        _ => format!("{count} saved servers. Select one to join directly."),
    };
    let top = heading(painter, content, "MULTIPLAYER", "Servers", &subtitle);
    let rem = painter.rem;
    let list = Rect {
        y: top + 3.2 * rem,
        h: content.y + content.h - top - 3.2 * rem,
        ..content
    };
    if count == 0 {
        empty(
            painter,
            Rect {
                h: (10.4 * rem).min(list.h),
                ..list
            },
        );
        return;
    }
    let shown = count.min(LISTED_SERVERS);
    let more = count - shown;
    let footer = if more > 0 {
        painter.line(oreui::CAPTION) + 1.2 * rem
    } else {
        0.0
    };
    let gap = 0.8 * rem;
    let row_h = ((list.h - footer - gap * (shown as f32 - 1.0)) / shown as f32)
        .min(6.4 * rem)
        .max(painter.line(oreui::BODY) + rem);
    let mut y = list.y;
    for (index, server) in overview.servers.iter().take(shown).enumerate() {
        let row = Rect {
            y,
            h: row_h,
            ..list
        };
        server_row(painter, row, index, server);
        y += row_h + gap;
    }
    if more > 0 {
        let note = format!("{more} more in the game's server list");
        painter.label(
            list.x,
            y + 0.4 * rem,
            oreui::CAPTION,
            theme::TEXT_FAINT,
            &note,
        );
    }
}

/// One row of the servers page: name and address on the left, last join and Join on the right.
fn server_row(painter: &mut Painter<'_>, row: Rect, index: usize, server: &Server) {
    let rem = painter.rem;
    let action = Action::Join(index);
    let lit = painter.hovered(action) || painter.style.focused == Some(action);
    painter.canvas.fill(
        row,
        if lit {
            theme::SURFACE_HOVER
        } else {
            theme::SURFACE
        },
    );
    if lit {
        painter.canvas.fill(
            Rect {
                w: 0.4 * rem,
                ..row
            },
            theme::ACCENT,
        );
    }
    let join_w = painter.width("JOIN", oreui::CAPTION);
    let right = row.x + row.w - 2.4 * rem;
    painter.label_in(
        row,
        right - join_w,
        oreui::CAPTION,
        if lit {
            theme::ACCENT_HOVER
        } else {
            theme::ACCENT
        },
        "JOIN",
    );
    let joined = server.joined.as_deref().unwrap_or("Never joined");
    let joined_w = painter.width(joined, oreui::CAPTION);
    let joined_x = right - join_w - 3.2 * rem - joined_w;
    painter.label_in(row, joined_x, oreui::CAPTION, theme::TEXT_DIM, joined);

    let x = row.x + 2.4 * rem;
    let room = joined_x - 2.4 * rem - x;
    let body = painter.line(oreui::BODY);
    let caption = painter.line(oreui::CAPTION);
    let name = painter.fit(&server.name, oreui::BODY, room);
    let address = painter.fit(&server.address, oreui::CAPTION, room);
    if row.h >= body + caption {
        let top = row.y + (row.h - body - caption) * 0.5;
        painter.label(x, top, oreui::BODY, theme::TEXT, &name);
        painter.label(x, top + body, oreui::CAPTION, theme::TEXT_FAINT, &address);
    } else {
        painter.label_in(row, x, oreui::BODY, theme::TEXT, &name);
    }
    painter.hit(action, row);
    painter.focus(action, row);
}

/// The installation's version, channel, resources and data folder, with a way to open it.
fn installation(painter: &mut Painter<'_>, content: Rect, overview: &Overview) {
    let top = heading(
        painter,
        content,
        "ABOUT THIS COPY",
        "Installation",
        "Where the game keeps its worlds, logs and settings, and what it will run.",
    );
    let rem = painter.rem;
    let row_h = 5.2 * rem;
    let rows = overview.rows();
    let card = Rect {
        y: top + 3.2 * rem,
        h: row_h * rows.len() as f32,
        ..content
    };
    painter.canvas.fill(card, theme::SURFACE);
    for (index, (label, value)) in rows.iter().enumerate() {
        let row = Rect {
            y: card.y + row_h * index as f32,
            h: row_h,
            ..card
        };
        if index > 0 {
            painter.canvas.fill(
                Rect {
                    x: row.x + 2.4 * rem,
                    w: row.w - 4.8 * rem,
                    h: oreui::EDGE * rem,
                    ..row
                },
                theme::DIVIDER,
            );
        }
        let x = row.x + 2.4 * rem;
        painter.label_in(row, x, oreui::BODY, theme::TEXT_DIM, label);
        let label_w = painter.width(label, oreui::BODY);
        let right = row.x + row.w - 2.4 * rem;
        let px = painter.px(oreui::BODY);
        let value =
            painter
                .text
                .truncate_middle(value, px, (right - x - label_w - 3.2 * rem).max(0.0));
        let ink = match *label {
            RESOURCES_ROW if overview.resources_ready => theme::READY,
            RESOURCES_ROW => theme::PENDING,
            _ => theme::TEXT,
        };
        let value_w = painter.width(&value, oreui::BODY);
        painter.label_in(row, right - value_w, oreui::BODY, ink, &value);
    }
    let button = Rect {
        x: content.x,
        y: card.y + card.h + 2.4 * rem,
        w: 22.0 * rem,
        h: 4.8 * rem,
    };
    painter.button(
        Action::OpenFolder,
        button,
        false,
        oreui::SECONDARY_BUTTON,
        "Open data folder",
    );
}
