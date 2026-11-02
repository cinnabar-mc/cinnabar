//! Modal, element-menu, and custom-form answers leave as their vanilla wire shapes.
use super::{FormValue, LocalFormAction, flush_form_response};
use crate::ui_runtime::UiRuntime;
use protocol::{
    CustomForm, CustomFormElement, CustomFormValue, ElementMenuForm, FormKind, FormRequestEvent,
    MenuElement, ModalDialogForm, ModalFormResponseSelection, Packet, ServerFormModel,
    custom_form_submit_response, modal_form_submit_response,
};
use std::sync::Arc;

fn admit(model: ServerFormModel, kind: FormKind) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id: 4,
            kind,
            title: None,
            json: Arc::from("{}"),
            model,
        },
        1,
        session,
        false,
    );
    runtime
}

fn sent(runtime: &mut UiRuntime) -> Vec<Packet> {
    let mut packets = Vec::new();
    while flush_form_response(
        runtime,
        |_| false,
        |packet| {
            packets.push(packet);
            Ok(())
        },
    )
    .unwrap()
    {}
    packets
}

fn answer(runtime: &mut UiRuntime, action: LocalFormAction) {
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime.respond_to_server_form(identity, action).unwrap();
}

#[test]
fn modal_buttons_answer_true_then_false() {
    for (button, choice) in [(0, true), (1, false)] {
        let mut runtime = admit(
            ServerFormModel::Modal(ModalDialogForm {
                title: protocol::FormText::from("T"),
                content: protocol::FormText::from("C"),
                button1: protocol::FormText::from("Yes"),
                button2: protocol::FormText::from("No"),
            }),
            FormKind::Modal,
        );
        answer(&mut runtime, LocalFormAction::SubmitButton(button));
        assert_eq!(
            sent(&mut runtime),
            vec![modal_form_submit_response(
                4,
                ModalFormResponseSelection::ModalButton(choice)
            )]
        );
    }
}

#[test]
fn element_menu_indexes_count_buttons_only() {
    let mut runtime = admit(
        ServerFormModel::ElementMenu(ElementMenuForm {
            title: protocol::FormText::from("T"),
            content: protocol::FormText::from("C"),
            elements: vec![
                MenuElement::Label(protocol::FormText::from("L")),
                MenuElement::Button {
                    text: protocol::FormText::from("A"),
                    image: None,
                },
            ]
            .into(),
        }),
        FormKind::Menu,
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    assert!(
        runtime
            .respond_to_server_form(identity, LocalFormAction::SubmitButton(1))
            .is_err(),
        "only one button exists"
    );
    answer(&mut runtime, LocalFormAction::SubmitButton(0));
    assert_eq!(
        sent(&mut runtime),
        vec![modal_form_submit_response(
            4,
            ModalFormResponseSelection::ButtonIndex(0)
        )]
    );
}

#[test]
fn control_locked_submission_keeps_every_original_value_slot() {
    let Some(protocol::WorldEvent::Ui(protocol::UiEvent::Form(event))) =
        protocol::into_world_event(
            protocol::test_support::packets::modal_form_request_packet(
                4,
                r#"{
            "type":"custom_form","content":[
                {"type":"label","text":"Values"},
                {"type":"toggle","default":true,"control_locked":"ingame"},
                {"type":"slider","min":0,"max":10,"default":4,"control_locked":"ingame"},
                {"type":"step_slider","steps":["A","B"],"default":1,"control_locked":"ingame"},
                {"type":"input","default":"Draft","control_locked":"ingame"}
            ]}"#,
            ),
            0,
        )
        .unwrap()
    else {
        panic!("custom form packet");
    };
    let mut runtime = admit(event.model, FormKind::Custom);
    answer(&mut runtime, LocalFormAction::CustomElements);
    assert_eq!(
        sent(&mut runtime),
        vec![custom_form_submit_response(
            4,
            &[
                CustomFormValue::Null,
                CustomFormValue::Toggle(true),
                CustomFormValue::Slider(4.0),
                CustomFormValue::Step(1),
                CustomFormValue::Input("Draft".into()),
            ]
        )]
    );
}

#[test]
fn custom_form_submits_the_edited_values_in_order() {
    let mut runtime = admit(
        ServerFormModel::Custom(CustomForm {
            icon: None,
            title: protocol::FormText::from("T"),
            elements: vec![
                CustomFormElement::Label {
                    text: protocol::FormText::from("L"),
                },
                CustomFormElement::Toggle {
                    control_locked: Default::default(),
                    text: protocol::FormText::from("On"),
                    default: false,
                    tooltip: None,
                },
                CustomFormElement::Input {
                    control_locked: Default::default(),
                    text: protocol::FormText::from("Name"),
                    placeholder: protocol::FormText::from(""),
                    default: Arc::from("Ste"),
                    tooltip: None,
                },
            ]
            .into(),
            submit: None,
        }),
        FormKind::Custom,
    );
    let engine = runtime.server_forms_mut().engine_mut();
    assert_eq!(engine.values[2], FormValue::Text("Ste".into()));
    engine.values[1] = FormValue::Toggle(true);
    engine.values[2] = FormValue::Text("Steve".into());
    answer(&mut runtime, LocalFormAction::CustomElements);
    assert_eq!(
        sent(&mut runtime),
        vec![custom_form_submit_response(
            4,
            &[
                CustomFormValue::Null,
                CustomFormValue::Toggle(true),
                CustomFormValue::Input("Steve".into())
            ]
        )]
    );
    assert!(
        runtime.server_forms().engine().values.is_empty(),
        "answering clears the live values"
    );
}

/// Supplies another request without changing the currently displayed form.
fn overlap(runtime: &mut UiRuntime) {
    let event = FormRequestEvent {
        form_id: 4,
        kind: FormKind::Menu,
        title: Some("Replacement".into()),
        json: "{}".into(),
        model: ServerFormModel::TextMenu(protocol::TextMenuForm {
            title: "Replacement".into(),
            content: "".into(),
            buttons: vec!["Next".into()].into(),
            button_images: [].into(),
            omitted_images: 0,
        }),
    };
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(event, 2, session, false);
}

#[test]
fn same_id_overlap_keeps_the_displayed_form_and_rejects_the_incoming_request() {
    let mut runtime = admit(
        ServerFormModel::Modal(ModalDialogForm {
            title: "Original".into(),
            content: "Stay here".into(),
            button1: "Yes".into(),
            button2: "No".into(),
        }),
        FormKind::Modal,
    );
    let original = runtime.server_forms().active().unwrap().clone();
    overlap(&mut runtime);
    assert_eq!(runtime.server_forms().active(), Some(&original));
    assert_eq!(sent(&mut runtime), [protocol::modal_form_busy_response(4)]);
    answer(&mut runtime, LocalFormAction::SubmitButton(0));
    assert_eq!(
        sent(&mut runtime),
        [modal_form_submit_response(
            4,
            ModalFormResponseSelection::ModalButton(true)
        )]
    );
}

#[test]
fn same_id_overlap_keeps_an_answer_until_backpressure_clears() {
    let mut runtime = admit(
        ServerFormModel::Modal(ModalDialogForm {
            title: "Original".into(),
            content: "Stay here".into(),
            button1: "Yes".into(),
            button2: "No".into(),
        }),
        FormKind::Modal,
    );
    answer(&mut runtime, LocalFormAction::SubmitButton(1));
    assert_eq!(
        flush_form_response(
            &mut runtime,
            |_| false,
            |_| Err(super::FormTransportError::Full)
        ),
        Err(super::FormTransportError::Full)
    );
    overlap(&mut runtime);
    assert!(runtime.server_forms().active().is_none());
    assert_eq!(
        sent(&mut runtime),
        [
            modal_form_submit_response(4, ModalFormResponseSelection::ModalButton(false)),
            protocol::modal_form_busy_response(4)
        ]
    );
    assert!(sent(&mut runtime).is_empty());
}

#[test]
fn answered_form_is_replaced_without_a_blank_handoff_frame() {
    let mut runtime = admit(
        ServerFormModel::Modal(ModalDialogForm {
            title: "Original".into(),
            content: "Stay here".into(),
            button1: "Next".into(),
            button2: "Close".into(),
        }),
        FormKind::Modal,
    );
    let old = runtime.server_forms().active().unwrap().clone();
    runtime
        .server_forms_mut()
        .respond_at(old.identity, LocalFormAction::SubmitButton(0), 1)
        .unwrap();
    assert_eq!(runtime.server_forms().active(), Some(&old));
    assert!(runtime.server_forms().handoff_pending());

    runtime
        .server_forms_mut()
        .advance_form_handoff(1 + super::FORM_HANDOFF_GRACE_MILLIS - 1);
    assert_eq!(runtime.server_forms().active(), Some(&old));

    overlap(&mut runtime);
    assert_eq!(
        runtime.server_forms().active().unwrap().title.as_deref(),
        Some("Replacement")
    );
    assert!(!runtime.server_forms().handoff_pending());
    assert_eq!(
        sent(&mut runtime),
        [modal_form_submit_response(
            4,
            ModalFormResponseSelection::ModalButton(true),
        )]
    );
}

#[test]
fn answered_form_closes_after_handoff_grace_without_a_replacement() {
    let mut runtime = admit(
        ServerFormModel::Modal(ModalDialogForm {
            title: "Original".into(),
            content: "Close after answering".into(),
            button1: "Done".into(),
            button2: "Cancel".into(),
        }),
        FormKind::Modal,
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime
        .server_forms_mut()
        .respond_at(identity, LocalFormAction::SubmitButton(0), 1)
        .unwrap();
    runtime
        .server_forms_mut()
        .advance_form_handoff(1 + super::FORM_HANDOFF_GRACE_MILLIS);
    assert!(runtime.server_forms().active().is_none());
    assert!(!runtime.server_forms().handoff_pending());
    assert!(
        runtime.server_forms().owns_input(),
        "the queued answer still owns input"
    );
    assert_eq!(
        sent(&mut runtime),
        [modal_form_submit_response(
            4,
            ModalFormResponseSelection::ModalButton(true),
        )]
    );
    assert!(!runtime.server_forms().owns_input());
}

#[test]
fn handoff_rejects_another_answer_after_the_first_response_is_sent() {
    let mut runtime = admit(
        ServerFormModel::Modal(ModalDialogForm {
            title: "Choose".into(),
            content: "One answer".into(),
            button1: "Next".into(),
            button2: "Close".into(),
        }),
        FormKind::Modal,
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime
        .respond_to_server_form_at(identity, LocalFormAction::SubmitButton(0), 1)
        .unwrap();
    assert_eq!(sent(&mut runtime).len(), 1);
    assert_eq!(
        runtime.respond_to_server_form_at(identity, LocalFormAction::SubmitButton(1), 2),
        Err(super::FormRespondError::PendingResponse),
    );
    assert_eq!(
        runtime.respond_to_server_form(identity, LocalFormAction::Dismiss),
        Err(super::FormRespondError::PendingResponse),
    );
    assert!(sent(&mut runtime).is_empty());
    assert!(runtime.server_forms().handoff_pending());
}

#[test]
fn server_closed_npc_retires_the_handoff() {
    let mut store = super::ServerFormStore::default();
    let mut event = FormRequestEvent {
        form_id: protocol::NPC_DIALOGUE_FORM_ID,
        kind: FormKind::NpcDialogue,
        title: None,
        json: "{}".into(),
        model: ServerFormModel::NpcDialogue(protocol::NpcDialogueForm {
            npc_unique_id: 7,
            scene_name: "Scene".into(),
            npc_name: "Guide".into(),
            dialogue: "Continue?".into(),
            buttons: Arc::from([protocol::NpcButton {
                text: "Next".into(),
                is_url: false,
                action_index: 0,
            }]),
            open: true,
        }),
    };
    store.admit(event.clone(), 1, 1, false);
    store
        .respond_at(
            store.active().unwrap().identity,
            LocalFormAction::SubmitButton(0),
            1,
        )
        .unwrap();
    let ServerFormModel::NpcDialogue(npc) = &mut event.model else {
        unreachable!()
    };
    npc.open = false;
    store.admit(event, 2, 1, false);
    assert!(store.active().is_none());
    assert!(!store.handoff_pending());
    assert!(!store.owns_input());
}
