//! Account guidance for receiving website selections in the local mod library.

use super::{Content, theme};
use launcher::menu::{MenuAction, auth::AuthState};
use ui_engine::engine::screen::PaintError;

/// Explains the current sign-in step without treating authentication as a successful sync.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let (title, body, action) = match &content.view.auth_state {
        AuthState::SignedOut => (
            "Sign in to sync your library",
            "Use the same Xbox account in Cinnabar and on the website. On a mod page, choose Add to Cinnabar.",
            Some(("Sign in", MenuAction::StartSignIn)),
        ),
        AuthState::Checking => (
            "Checking your account…",
            "Cinnabar is checking your Xbox account. Local imports remain available.",
            None,
        ),
        AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. } => (
            "Finish signing in",
            "Finish Microsoft sign-in in the browser, then return here. Use the same Xbox account on the marketplace.",
            Some(("Continue sign-in", MenuAction::OpenSignInLink)),
        ),
        AuthState::Failed(_) => (
            "Sign in to reconnect your library",
            "Sign in again to receive mods from the marketplace. Local imports remain available.",
            Some(("Sign in again", MenuAction::StartSignIn)),
        ),
        AuthState::Authenticated => (
            "Signed in to Cinnabar",
            "Choose Add to Cinnabar on the website using the same Xbox account. Verified downloads appear in My mods.",
            None,
        ),
    };
    let permissions = "Downloaded mods stay disabled until you enable them and review permissions.";
    let [left, right] = content.inset();
    let pad = content.canvas.r(1.6);
    let gap = content.canvas.r(0.8);
    let width = (right - left - 2.0 * pad).max(1.0);
    let title_height = content
        .canvas
        .measure_height(title, width, theme::SECTION_HEADER)?;
    let body_height = content.canvas.measure_height(body, width, theme::CAPTION)?;
    let permissions_height = content
        .canvas
        .measure_height(permissions, width, theme::CAPTION)?;
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
    let action_height = action_size.map_or(0.0, |[_, height]| height + gap);
    let bottom = content.y
        + 2.0 * pad
        + title_height
        + body_height
        + permissions_height
        + 2.0 * gap
        + action_height;
    let bounds = [left, content.y, right, bottom];
    content.canvas.fill(bounds, theme::NEUTRAL80.fill)?;
    content.canvas.frame(bounds, theme::EDGE, theme::BORDER)?;
    let mut y = content.y + pad;
    content.canvas.text(
        title,
        [left + pad, y],
        width,
        theme::SECTION_HEADER,
        theme::TEXT,
        false,
    )?;
    y += title_height + gap;
    content.canvas.text(
        body,
        [left + pad, y],
        width,
        theme::CAPTION,
        theme::TEXT_DIMMER,
        false,
    )?;
    y += body_height + gap;
    content.canvas.text(
        permissions,
        [left + pad, y],
        width,
        theme::CAPTION,
        theme::TEXT_DIMMER,
        false,
    )?;
    if let Some(((label, action), [button_width, height])) = action.zip(action_size) {
        y += permissions_height + gap;
        crate::widgets::wrapped_button(
            content.canvas,
            content.view,
            [left + pad, y, left + pad + button_width, y + height],
            crate::widgets::Variant::Secondary,
            label,
            Some(action),
        )?;
    }
    content.y = bottom + content.canvas.r(1.2);
    Ok(())
}
