//! The merged native presentation parameters mods set, read by the native renderers.

use std::sync::Arc;
use ui::mod_presentation::{ChatAction, NametagStyle, Presentation};

impl super::ModUi {
    /// Replaces the merged parameters; returns whether the clickable chat spans changed, so the
    /// caller can revoke chat state bound to the old ones.
    pub fn set_mod_presentation(&mut self, presentation: Arc<Presentation>) -> bool {
        if Arc::ptr_eq(&self.mod_presentation, &presentation) {
            return false;
        }
        let chat_changed = self.mod_presentation.chat_actions != presentation.chat_actions;
        self.mod_presentation = presentation;
        chat_changed
    }

    /// The merged parameters every native renderer reads.
    pub fn mod_presentation(&self) -> &Arc<Presentation> {
        &self.mod_presentation
    }

    /// The nametag renderer's parameters; none keeps native nametags.
    pub fn mod_nametag_style(&self) -> Option<&NametagStyle> {
        self.mod_presentation.nametag_style.as_ref()
    }

    /// The literal chat spans that report a click to their owning mod.
    pub fn mod_chat_actions(&self) -> &[ChatAction] {
        &self.mod_presentation.chat_actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a change of the clickable spans asks the caller to revoke chat state.
    #[test]
    fn presentation_reports_chat_action_changes() {
        let mut ui = super::super::ModUi::default();
        let action = ChatAction {
            text: "Help".into(),
            action: "help".into(),
        };
        let first = Arc::new(Presentation {
            chat_actions: vec![action.clone()],
            ..Default::default()
        });
        assert!(ui.set_mod_presentation(Arc::clone(&first)));
        assert!(!ui.set_mod_presentation(Arc::clone(&first)));
        let restyled = Arc::new(Presentation {
            chat_actions: vec![action],
            nametag_style: Some(NametagStyle::default()),
            ..Default::default()
        });
        assert!(!ui.set_mod_presentation(restyled));
        assert!(ui.mod_nametag_style().is_some());
        assert!(ui.set_mod_presentation(Arc::default()));
        assert!(ui.mod_chat_actions().is_empty());
    }
}
