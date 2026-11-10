//! Shared Play tab order and entry actions.

use super::{MenuAction, MenuScreen, MenuView};

/// The Play screen's tabs in tab-bar order: Worlds, Realms and Servers.
pub const PLAY_TABS: [MenuScreen; 3] = [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers];

/// Joining the featured-list entry at `index`.
pub fn play_featured(view: &MenuView, index: usize) -> Option<MenuAction> {
    (index < view.featured.len()).then_some(MenuAction::PlayFeatured(index))
}
