//! Presents a packet owner's generic panel surface through the shared JSON-UI path.
use super::packets::PacketMods;
use client_shell::ui_resources::UiPresentationResource;
use mod_ui::PanelUiOwner;

#[derive(Default)]
pub(super) struct State {
    pub owned: bool,
    pub pending: mod_api::panel_input::Input,
    pub owner: Option<PanelUiOwner>,
    revision: u64,
    handle: Option<mod_host::packet_mod::panel::PanelHandle>,
}
/// Exchanges a physical sample with the session mailbox without executing guest code.
pub(super) fn poll(
    packets: &mut PacketMods,
    presentation: &mut UiPresentationResource,
    viewport: Option<[f32; 2]>,
    mut extension: Option<&mut super::ModRuntime>,
) {
    let Some(handle) = packets.panel_handle() else {
        packets.panel.retire(presentation);
        packets.panel = State::default();
        return;
    };
    if packets
        .panel
        .handle
        .as_ref()
        .is_none_or(|old| !old.same_instance(&handle))
    {
        packets.panel.retire(presentation);
        packets.panel = State {
            owner: Some(PanelUiOwner::PacketSession(handle.identity())),
            handle: Some(handle.clone()),
            ..Default::default()
        };
    }
    let mut input = std::mem::take(&mut packets.panel.pending);
    input.open = packets.panel.owned && presentation.mod_ui().mod_panel_open();
    input.viewport = viewport;
    if packets.panel.owned && !input.open {
        handle.close();
        packets.panel.owned = false;
    }
    let output = handle.exchange(input);
    let newer_frame = extension.as_ref().is_some_and(|runtime| {
        (0..runtime.hosts.host_count()).any(|index| {
            let host = runtime.hosts.host(index);
            host.panel_open() && host.panel_open_order() > output.open_order
        })
    });
    if newer_frame {
        handle.close();
        packets.panel.owned = false;
        return;
    }
    if output.revision != packets.panel.revision {
        packets.panel.revision = output.revision;
        if output.open {
            if let Some(panel) = output.panel.as_ref() {
                let owner = PanelUiOwner::PacketSession(handle.identity());
                if let Err(error) = presentation.mod_ui_mut().set_mod_panel(owner, Some(panel)) {
                    bevy::log::warn!(%error, "packet panel rejected");
                    handle.close();
                    let _ = presentation.mod_ui_mut().set_mod_panel(owner, None);
                    packets.panel.owned = false;
                    return;
                }
                if let Some(runtime) = extension.as_mut() {
                    for index in 0..runtime.hosts.host_count() {
                        runtime.hosts.host_mut(index).set_panel_open(false);
                    }
                }
                presentation.mod_ui_mut().set_mod_panel_open(true);
                packets.panel.owned = true;
            }
        } else if packets.panel.owned {
            presentation.mod_ui_mut().set_mod_panel_open(false);
            packets.panel.owned = false;
        }
    }
}

impl State {
    /// Retires only this session's surface, preserving a newer frame owner's panel.
    fn retire(&mut self, presentation: &mut UiPresentationResource) {
        self.owned = false;
        if let Some(owner) = self.owner.take() {
            let _ = presentation.mod_ui_mut().set_mod_panel(owner, None);
            presentation.mod_ui_mut().set_prepared_panel_ui(owner, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retiring_a_session_releases_its_surface_and_preserves_a_newer_owner() {
        let panel: ui::mod_panel::Panel = serde_json::from_value(serde_json::json!({
            "controls": [], "surface": {"screen":"fixture.root",
            "document":"{\"namespace\":\"fixture\",\"root\":{\"type\":\"panel\"}}"}
        }))
        .unwrap();
        let session = PanelUiOwner::PacketSession(1);
        let frame = PanelUiOwner::Environment(0);
        let mut presentation =
            UiPresentationResource(client_ui::test_support::mini_engine_presentation());
        for current in [session, frame] {
            presentation
                .mod_ui_mut()
                .set_mod_panel(current, Some(&panel))
                .unwrap();
            presentation.mod_ui_mut().set_mod_panel_open(true);
            let mut state = State {
                owner: Some(session),
                owned: true,
                ..Default::default()
            };
            state.retire(&mut presentation);
            assert!(!state.owned);
            assert_eq!(presentation.mod_ui().mod_panel_open(), current == frame);
        }
    }
}
