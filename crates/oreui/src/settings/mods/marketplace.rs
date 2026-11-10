//! Compact account guidance for receiving website selections in the local mod library.

use super::{Content, theme};
use launcher::menu::{MenuAction, auth::AuthState};
use ui_engine::engine::screen::PaintError;

/// Shows one account hint without treating authentication as a successful library sync.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let (message, action) = match &content.view.auth_state {
        AuthState::SignedOut => (
            "Use the same Xbox account as the website to sync mods.",
            Some(("Sign in", MenuAction::StartSignIn)),
        ),
        AuthState::Checking => ("Checking your account…", None),
        AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. } => (
            "Finish signing in with your browser.",
            Some(("Continue sign-in", MenuAction::OpenSignInLink)),
        ),
        AuthState::Failed(_) => (
            "Sign in again to sync mods.",
            Some(("Sign in again", MenuAction::StartSignIn)),
        ),
        AuthState::Authenticated => (
            "Choose Add to Cinnabar on the website. Downloads appear in My mods.",
            None,
        ),
    };
    let [left, right] = content.inset();
    let width = (right - left).max(1.0);
    let gap = content.canvas.r(1.2);
    let action_size = action
        .map(|(label, _)| {
            crate::widgets::wrapped_button_size(
                content.canvas,
                width,
                crate::widgets::Variant::Secondary,
                label,
            )
        })
        .transpose()?;
    let inline_width = action_size
        .map(|[button_width, _]| width - button_width - gap)
        .filter(|&text_width| text_width >= content.canvas.r(24.0));
    let inline = inline_width.is_some();
    let text_width = inline_width.unwrap_or(width);
    let text_height = content
        .canvas
        .measure_height(message, text_width, theme::CAPTION)?;
    let height = action_size.map_or(text_height, |[_, button_height]| {
        if inline {
            text_height.max(button_height)
        } else {
            text_height + gap + button_height
        }
    });
    content.canvas.text(
        message,
        [
            left,
            content.y
                + if inline {
                    (height - text_height) * 0.5
                } else {
                    0.0
                },
        ],
        text_width,
        theme::CAPTION,
        theme::TEXT_DIMMER,
        false,
    )?;
    if let Some(((label, action), [button_width, button_height])) = action.zip(action_size) {
        let x = if inline { right - button_width } else { left };
        let y = content.y
            + if inline {
                (height - button_height) * 0.5
            } else {
                text_height + gap
            };
        crate::widgets::wrapped_button(
            content.canvas,
            content.view,
            [x, y, x + button_width, y + button_height],
            crate::widgets::Variant::Secondary,
            label,
            Some(action),
        )?;
    }
    content.y += height + content.canvas.r(1.2);
    Ok(())
}
