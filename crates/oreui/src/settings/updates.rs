//! App and mod update preferences share a settings tab with app worker status.

use super::{Content, services};
use launcher::menu::{
    MenuAction,
    settings_options::{SettingId, layout::SettingPanel},
    updates::{Action, Ready, Status, build_title, installed_version},
};
use ui_engine::engine::screen::PaintError;

/// Groups app update controls and independently available mod update preferences.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("Updates", "")?;
    app(content)?;
    content.heading("Mods", "")?;
    for option in SettingPanel::ModUpdates.options() {
        content.option(option.id)?;
    }
    Ok(())
}

/// Presents app preferences and worker actions only when this window owns the updater.
fn app(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    if !view.updates.active {
        let message = if view.updates.manual_pending {
            view.updates.status_text()
        } else {
            "App update controls are unavailable in this window.".into()
        };
        return content.label_text(&installed_version(&view.updates.running), &message);
    }
    let channel = view.settings_options.update_preferences().channel;
    content.label_text(
        &installed_version(&view.updates.running),
        &format!("Following {}. {}", channel.title(), channel.description()),
    )?;
    content.option(SettingId::UpdateChannel)?;
    let mut status = view.updates.status_text();
    if view.over_world {
        status.push_str(" Return to the home menu to check or install updates.");
    }
    let target = view
        .updates
        .target
        .as_ref()
        .map_or_else(|| "App updates".into(), build_title);
    let ready = (!view.over_world)
        .then_some(view.updates.ready.as_ref())
        .flatten();
    let ready_primary =
        ready.filter(|_| !view.updates.status.is_busy() && view.updates.status != Status::Failed);
    if let Some(ready) = ready_primary {
        install(content, ready)?;
    }
    services::action_row_text(
        content,
        if ready_primary.is_some() {
            "Check for newer releases"
        } else {
            &target
        },
        if ready_primary.is_some() { "" } else { &status },
        "Check now",
        (!view.over_world && !view.updates.status.is_busy())
            .then_some(MenuAction::Update(Action::CheckNow)),
    )?;
    if view.updates.status == Status::Downloading
        && let Some(progress) = view.updates.download
        && progress.total_bytes > 0
    {
        let tr = |key: &str, fallback: &str| {
            (content.translate)(key).map_or_else(|| fallback.to_owned(), |value| value.to_string())
        };
        let text = format!(
            "{} / {}",
            launcher::download::file_size(progress.received_bytes, &tr),
            launcher::download::file_size(progress.total_bytes, &tr)
        );
        content.label_text("", &text)?;
        let [left, right] = content.inset();
        let height = content.canvas.r(super::super::theme::PROGRESS_HEIGHT);
        crate::progress::track(
            content.canvas,
            [left, content.y, right, content.y + height],
            Some(
                progress.received_bytes.min(progress.total_bytes) as f32
                    / progress.total_bytes as f32,
            ),
        )?;
        content.y += height + content.canvas.r(0.8);
    }
    if !view.over_world {
        if ready_primary.is_none()
            && let Some(ready) = ready
        {
            install(content, ready)?;
        }
        if !view.updates.status.is_busy()
            && let Some(message) = &view.updates.message
        {
            content.label_text("", message)?;
        }
        if view.updates.switch_available && !view.updates.status.is_busy() {
            let release_channel = view
                .updates
                .target
                .as_ref()
                .and_then(|release| release.channel)
                .unwrap_or(channel);
            let label = format!("Install {} now", release_channel.title());
            services::action_row_text(
                content,
                &format!(
                    "{} is older than your current build",
                    release_channel.title()
                ),
                "Download this release now, then restart to switch channels.",
                &label,
                Some(MenuAction::Update(Action::SwitchNow)),
            )?;
            if view.updates.waiting_for_channel {
                content.label_text("Keeping your current build",
                    &format!("Waiting for the next newer {} release. You can still install the older release above.", channel.title()))?;
            } else {
                services::action_row_text(
                    content,
                    "Keep your current build",
                    "Stay on this build until the selected channel publishes a newer release.",
                    &format!("Wait for next {}", channel.title()),
                    Some(MenuAction::Update(Action::WaitForChannel)),
                )?;
            }
        }
    }
    for option in SettingPanel::Updates
        .options()
        .filter(|option| option.id != SettingId::UpdateChannel)
    {
        content.option(option.id)?;
    }
    Ok(())
}

/// Keeps the verified release and its installation action together without repeating its identity.
fn install(content: &mut Content<'_, '_>, ready: &Ready) -> Result<(), PaintError> {
    services::action_row_text(
        content,
        &build_title(&ready.release),
        if ready.writable {
            "Ready to install. Restart when you're ready."
        } else {
            "Finish installation with the downloaded package."
        },
        ready.action_label(),
        Some(MenuAction::Update(ready.action())),
    )
}

#[cfg(test)]
mod tests;
