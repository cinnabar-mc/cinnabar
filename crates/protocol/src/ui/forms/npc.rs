//! NPC dialogue and server settings, both carried through the server-form
//! lifecycle. An NPC dialogue opens as a form under a reserved id whose buttons
//! are the action list's button-mode entries (keeping their action indexes) and
//! answers with `NpcRequest`; a server settings response is a custom form marked
//! [`FormKind::ServerSettings`], answered like any form.

use std::sync::Arc;

use bedrock_protocol_1_26_60::enums::{NpcDialogueActionType, RequestType};
use bedrock_protocol_1_26_60::packets::{
    ModalFormRequest, NpcDialogue, NpcRequest, ServerSettingsRequest, ServerSettingsResponse,
};
use bedrock_protocol_1_26_60::types::ActorRuntimeID;
use bedrock_protocol_1_26_60::wire;
use serde_json::Value;

use super::{FormKind, FormRequestEvent, ServerFormModel, UnsupportedForm, normalize_form};
use crate::ui::{MAX_FORM_JSON_BYTES, MAX_UI_TEXT_BYTES, UiEvent, UiPacketError};

/// The form id NPC dialogues occupy; server forms never answer to it.
pub const NPC_DIALOGUE_FORM_ID: u32 = u32::MAX;
/// NPC action entries whose `mode` makes them a clickable button.
const BUTTON_MODE: u64 = 0;
const URL_ACTION_TYPE: u64 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcDialogueForm {
    /// Persistent ID from the dialogue; replies resolve its live actor.
    pub npc_unique_id: i64,
    pub scene_name: Arc<str>,
    pub npc_name: Arc<str>,
    pub dialogue: Arc<str>,
    pub buttons: Arc<[NpcButton]>,
    /// `false` for the server closing the dialogue.
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcButton {
    pub text: Arc<str>,
    /// Selects the URL label style rather than the command label style.
    pub is_url: bool,
    /// Index into the full action list, echoed back in the request.
    pub action_index: u8,
}

/// What an NPC request asks the server to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcRequestKind {
    ExecuteAction(u8),
    ExecuteOpeningCommands,
    ExecuteClosingCommands,
}

pub(crate) fn normalize_npc_dialogue(packet: NpcDialogue) -> Result<UiEvent, UiPacketError> {
    let open = !matches!(
        packet.npc_dialogue_action_type,
        NpcDialogueActionType::Close
    );
    let action_json = packet.action_json.to_string_lossy();
    if action_json.len() > MAX_FORM_JSON_BYTES {
        return Err(UiPacketError::FormTooLarge {
            bytes: action_json.len(),
            max: MAX_FORM_JSON_BYTES,
        });
    }
    let dialogue = packet.dialogue.to_string_lossy();
    let npc_name = packet.npc_name.to_string_lossy();
    let scene_name = packet.scene_name.to_string_lossy();
    let model = if [&dialogue, &npc_name, &scene_name]
        .iter()
        .any(|text| text.len() > MAX_UI_TEXT_BYTES)
    {
        ServerFormModel::Unsupported(UnsupportedForm::Limit)
    } else {
        match buttons(&action_json) {
            Ok(buttons) => ServerFormModel::NpcDialogue(NpcDialogueForm {
                npc_unique_id: packet.npc_id_raw_id.0 as i64,
                scene_name: Arc::from(&*scene_name),
                npc_name: Arc::from(&*npc_name),
                dialogue: Arc::from(&*dialogue),
                buttons: buttons.into(),
                open,
            }),
            Err(reason) => ServerFormModel::Unsupported(reason),
        }
    };
    Ok(UiEvent::Form(FormRequestEvent {
        form_id: NPC_DIALOGUE_FORM_ID,
        kind: FormKind::NpcDialogue,
        title: Some(Arc::from(&*npc_name)),
        json: Arc::from(&*action_json),
        model,
    }))
}

/// The NPC actor's synced dialogue data: its name tag, `InteractText` and
/// `Actions` list. Vanilla servers keep an NPC's dialogue there and open the
/// screen with an `NpcDialogue` packet that names no scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NpcActorDialogue<'a> {
    pub name: Option<&'a str>,
    pub interact_text: Option<&'a str>,
    pub actions_json: Option<&'a str>,
}

impl FormRequestEvent {
    /// Chooses an opening NPC dialogue's source as the vanilla client does: with
    /// an empty scene name the name, text and action list all come from `actor`
    /// and the packet's own fields are ignored; with a scene name the packet's
    /// fields stay. Oversized actor text leaves that field empty, and an
    /// oversized or malformed action list leaves no buttons.
    pub fn use_npc_actor_dialogue(&mut self, actor: NpcActorDialogue<'_>) {
        let ServerFormModel::NpcDialogue(npc) = &mut self.model else {
            return;
        };
        if !npc.open || !npc.scene_name.is_empty() {
            return;
        }
        let text = |value: Option<&str>| {
            Arc::from(
                value
                    .filter(|text| text.len() <= MAX_UI_TEXT_BYTES)
                    .unwrap_or(""),
            )
        };
        npc.npc_name = text(actor.name);
        self.title = Some(Arc::clone(&npc.npc_name));
        npc.dialogue = text(actor.interact_text);
        let json = actor
            .actions_json
            .filter(|json| json.len() <= MAX_FORM_JSON_BYTES)
            .unwrap_or("");
        let (json, buttons) = match buttons(json) {
            Ok(buttons) => (json, buttons),
            Err(_) => ("", Vec::new()),
        };
        npc.buttons = buttons.into();
        self.json = Arc::from(json);
    }
}

/// Button-mode actions from the action list; an empty list is no buttons.
fn buttons(json: &str) -> Result<Vec<NpcButton>, UnsupportedForm> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(json).map_err(|_| UnsupportedForm::Controls)?;
    let actions = value.as_array().ok_or(UnsupportedForm::Controls)?;
    if actions.len() > usize::from(u8::MAX) + 1 {
        return Err(UnsupportedForm::Limit);
    }
    let mut buttons = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let Some(action) = action.as_object() else {
            continue;
        };
        // A missing mode is a button, as the vanilla editor writes them.
        if action
            .get("mode")
            .and_then(Value::as_u64)
            .unwrap_or(BUTTON_MODE)
            != BUTTON_MODE
        {
            continue;
        }
        let text = action
            .get("button_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if text.len() > MAX_UI_TEXT_BYTES {
            return Err(UnsupportedForm::Limit);
        }
        buttons.push(NpcButton {
            text: Arc::from(text),
            is_url: action.get("type").and_then(Value::as_u64) == Some(URL_ACTION_TYPE),
            action_index: index as u8,
        });
    }
    Ok(buttons)
}

/// Encodes an NPC request for `npc_runtime_id` in `scene_name`.
pub fn npc_request_packet(
    npc_runtime_id: u64,
    scene_name: &str,
    kind: NpcRequestKind,
) -> crate::Packet {
    let (request_type, action_index) = match kind {
        NpcRequestKind::ExecuteAction(index) => (RequestType::ExecuteAction, index),
        NpcRequestKind::ExecuteOpeningCommands => (RequestType::ExecuteOpeningCommands, 0),
        NpcRequestKind::ExecuteClosingCommands => (RequestType::ExecuteClosingCommands, 0),
    };
    crate::Packet::new(NpcRequest {
        npc_runtime_id: ActorRuntimeID(npc_runtime_id),
        request_type,
        actions: wire::Str::default(),
        action_index: wire::U8(action_index),
        scene_name: scene_name.into(),
    })
}

pub(crate) fn normalize_server_settings(
    packet: ServerSettingsResponse,
) -> Result<UiEvent, UiPacketError> {
    let mut event = normalize_form(ModalFormRequest {
        form_id: packet.form_id,
        form_ui_json: packet.form_ui_json,
    })?;
    if let UiEvent::Form(form) = &mut event {
        form.kind = FormKind::ServerSettings;
    }
    Ok(event)
}

/// Asks the server for its settings form (sent when the settings screen opens).
pub fn server_settings_request_packet() -> crate::Packet {
    crate::Packet::new(ServerSettingsRequest {})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(packet: NpcDialogue) -> FormRequestEvent {
        match normalize_npc_dialogue(packet).unwrap() {
            UiEvent::Form(form) => form,
            other => panic!("expected a form, got {other:?}"),
        }
    }

    fn npc(form: &FormRequestEvent) -> &NpcDialogueForm {
        match &form.model {
            ServerFormModel::NpcDialogue(npc) => npc,
            other => panic!("expected an NPC dialogue, got {other:?}"),
        }
    }

    const ACTOR: NpcActorDialogue<'static> = NpcActorDialogue {
        name: Some("NPC"),
        interact_text: Some("Welcome!"),
        actions_json: Some(r#"[{"button_name":"Shop","mode":0,"type":1}]"#),
    };

    #[test]
    fn sceneless_dialogue_shows_actor_data() {
        // BDS and Realms open a talked-to NPC with every text field empty.
        let mut form = opened(NpcDialogue::default());
        form.use_npc_actor_dialogue(ACTOR);
        let npc = npc(&form);
        assert_eq!(npc.npc_name.as_ref(), "NPC");
        assert_eq!(form.title.as_deref(), Some("NPC"));
        assert_eq!(npc.dialogue.as_ref(), "Welcome!");
        assert_eq!(npc.buttons.len(), 1);
        assert_eq!(npc.buttons[0].text.as_ref(), "Shop");
        assert_eq!(npc.buttons[0].action_index, 0);
        assert_eq!(Some(form.json.as_ref()), ACTOR.actions_json);
    }

    #[test]
    fn sceneless_dialogue_replaces_packet_fields_with_actor_data() {
        let mut form = opened(NpcDialogue {
            npc_name: "Guide".into(),
            dialogue: "Packet text".into(),
            action_json: r#"[{"button_name":"Quest"},{"button_name":"Trade"}]"#.into(),
            ..NpcDialogue::default()
        });
        form.use_npc_actor_dialogue(NpcActorDialogue {
            interact_text: None,
            ..ACTOR
        });
        let npc = npc(&form);
        assert_eq!(npc.npc_name.as_ref(), "NPC");
        assert!(npc.dialogue.is_empty());
        assert_eq!(npc.buttons.len(), 1);
        assert_eq!(npc.buttons[0].text.as_ref(), "Shop");
    }

    #[test]
    fn scene_dialogue_keeps_packet_fields() {
        let mut form = opened(NpcDialogue {
            scene_name: "intro".into(),
            npc_name: "Guide".into(),
            dialogue: "Packet text".into(),
            action_json: r#"[{"button_name":"Quest"}]"#.into(),
            ..NpcDialogue::default()
        });
        form.use_npc_actor_dialogue(ACTOR);
        let npc = npc(&form);
        assert_eq!(npc.npc_name.as_ref(), "Guide");
        assert_eq!(npc.dialogue.as_ref(), "Packet text");
        assert_eq!(npc.buttons[0].text.as_ref(), "Quest");
    }

    #[test]
    fn scene_dialogue_with_empty_fields_ignores_actor_data() {
        let mut form = opened(NpcDialogue {
            scene_name: "intro".into(),
            ..NpcDialogue::default()
        });
        form.use_npc_actor_dialogue(ACTOR);
        let npc = npc(&form);
        assert!(npc.npc_name.is_empty());
        assert!(npc.dialogue.is_empty());
        assert!(npc.buttons.is_empty());
    }

    #[test]
    fn malformed_actor_actions_leave_the_dialogue_without_buttons() {
        let mut form = opened(NpcDialogue::default());
        form.use_npc_actor_dialogue(NpcActorDialogue {
            actions_json: Some("{}"),
            ..ACTOR
        });
        assert!(npc(&form).buttons.is_empty());
        assert!(form.json.is_empty());
        assert_eq!(npc(&form).dialogue.as_ref(), "Welcome!");
    }

    #[test]
    fn closing_dialogue_ignores_actor_data() {
        let mut form = opened(NpcDialogue {
            npc_dialogue_action_type: NpcDialogueActionType::Close,
            ..NpcDialogue::default()
        });
        form.use_npc_actor_dialogue(ACTOR);
        assert!(npc(&form).dialogue.is_empty());
    }

    #[test]
    fn action_buttons_preserve_their_label_kind() {
        let parsed = buttons(r#"[{"button_name":"Command","type":1,"mode":0},{"button_name":"Website","type":0,"mode":0},{"button_name":"Default"}]"#).unwrap();
        assert!(!parsed[0].is_url);
        assert!(parsed[1].is_url);
        assert!(!parsed[2].is_url);
    }

    #[test]
    fn button_mode_actions_keep_their_action_indexes() {
        let json = r#"[{"button_name":"Shop","mode":0},{"button_name":"","mode":1},{"button_name":"Quest"}]"#;
        let parsed = buttons(json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].text.as_ref(), "Quest");
        assert_eq!(parsed[1].action_index, 2);
        assert!(buttons("").unwrap().is_empty());
        assert_eq!(buttons("{}"), Err(UnsupportedForm::Controls));
    }
}
