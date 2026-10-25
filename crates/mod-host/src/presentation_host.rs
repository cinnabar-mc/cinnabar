//! The `presentation` imports: retained parameters of native world presentation, published only
//! after a successful callback. The client merges every mod's output in load order.

use super::{
    MAX_IMPORT_WRITES, State,
    cinnabar::extension::{presentation as wit, render::Rgba},
};
use anyhow::{Result, bail};
use std::{collections::BTreeMap, sync::Arc};
use ui::mod_presentation::{
    Annotation, ChatAction, HurtOverlay, MAX_ACTOR_TINTS, MAX_ANNOTATIONS, MAX_CHAT_ACTIONS,
    MAX_TOOLTIP_LINES, ModPresentation, NametagStyle, Span, SpanPosition, Spans, TooltipLine,
    validate_color,
};

// The guest-facing limits in `mod-api` are the ones the shared presentation types enforce.
const _: () = assert!(
    mod_api::presentation::MAX_ANNOTATIONS == MAX_ANNOTATIONS
        && mod_api::presentation::MAX_ANNOTATION_SPANS
            == ui::mod_presentation::MAX_ANNOTATION_SPANS
        && mod_api::presentation::MAX_ANNOTATION_SPAN_BYTES
            == ui::mod_presentation::MAX_ANNOTATION_SPAN_BYTES
        && mod_api::presentation::MAX_ACTOR_TINTS == MAX_ACTOR_TINTS
        && mod_api::presentation::MAX_TOOLTIP_LINES == MAX_TOOLTIP_LINES
        && mod_api::presentation::MAX_ITEM_TOOLTIP_LINES
            == ui::mod_presentation::MAX_ITEM_TOOLTIP_LINES
        && mod_api::presentation::MAX_TOOLTIP_LINE_BYTES
            == ui::mod_presentation::MAX_TOOLTIP_LINE_BYTES
        && mod_api::presentation::MAX_CHAT_ACTIONS == MAX_CHAT_ACTIONS
        && mod_api::presentation::MAX_CHAT_ACTION_TEXT_BYTES
            == ui::mod_presentation::MAX_CHAT_ACTION_TEXT_BYTES
        && mod_api::presentation::MAX_CHAT_ACTION_ID_BYTES == ui::mod_panel::MAX_PANEL_ID_BYTES
);

/// One mod's committed presentation, its callback's staged copy and its write allowance.
#[derive(Default)]
pub(super) struct PresentationState {
    committed: ModPresentation,
    pending: Option<ModPresentation>,
    revision: u64,
    writes: u32,
}

impl PresentationState {
    /// Starts a callback with nothing staged and a fresh write allowance.
    pub fn begin(&mut self) {
        self.pending = None;
        self.writes = 0;
    }

    /// Publishes staged parameters, giving changed output a process-unique revision.
    pub fn commit(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        if let Some(pending) = self.pending.take()
            && pending != self.committed
        {
            self.committed = pending;
            self.revision = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The last committed parameters and their revision.
    pub fn output(&self) -> (&ModPresentation, u64) {
        (&self.committed, self.revision)
    }

    /// The callback's working copy, starting from the committed parameters.
    fn stage(&mut self) -> &mut ModPresentation {
        self.pending.get_or_insert_with(|| self.committed.clone())
    }
}

/// Charges one presentation write, then checks hud authority and, when `actor_rows` is set,
/// players or entities authority.
fn admit(state: &mut State, actor_rows: bool) -> Result<Result<(), String>> {
    state.presentation.writes += 1;
    if state.presentation.writes > MAX_IMPORT_WRITES {
        bail!("presentation import budget exhausted");
    }
    Ok(if !state.grants.hud {
        Err("HUD capability denied".into())
    } else if actor_rows && !state.grants.players && !state.grants.entities {
        Err("actor observation capability denied".into())
    } else {
        Ok(())
    })
}

/// Copies a WIT colour into channel order.
fn rgba(color: Rgba) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

/// Validates an optional WIT colour.
fn color(color: Option<Rgba>) -> Result<Option<[f32; 4]>, String> {
    color
        .map(rgba)
        .map(|value| validate_color(value).map(|()| value))
        .transpose()
}

/// Converts one surface's spans; an empty list draws nothing on that surface.
fn spans(
    spans: Vec<wit::TextSpan>,
    position: wit::AnnotationPosition,
) -> Result<Option<Arc<Spans>>, String> {
    if spans.is_empty() {
        return Ok(None);
    }
    let spans = Spans {
        spans: spans
            .into_iter()
            .map(|span| Span {
                text: span.text.into(),
                color: rgba(span.color),
            })
            .collect(),
        position: match position {
            wit::AnnotationPosition::Before => SpanPosition::Before,
            wit::AnnotationPosition::After => SpanPosition::After,
            wit::AnnotationPosition::Above => SpanPosition::Above,
            wit::AnnotationPosition::Below => SpanPosition::Below,
        },
    };
    spans.validate()?;
    Ok(Some(Arc::new(spans)))
}

/// Requires distinct nonzero runtime IDs within a bound.
fn keyed<T>(rows: Vec<(u64, T)>, limit: usize) -> Result<BTreeMap<u64, T>, String> {
    if rows.len() > limit {
        return Err("too many actor rows".into());
    }
    let count = rows.len();
    let rows: BTreeMap<_, _> = rows.into_iter().collect();
    if rows.len() != count || rows.contains_key(&0) {
        return Err("actor rows need distinct nonzero runtime IDs".into());
    }
    Ok(rows)
}

/// Converts and validates every annotation, dropping those with no spans on either surface.
fn annotations(annotations: Vec<wit::Annotation>) -> Result<BTreeMap<u64, Annotation>, String> {
    let rows = annotations
        .into_iter()
        .map(|annotation| {
            Ok((
                annotation.runtime_id,
                Annotation {
                    nametag: spans(annotation.nametag, annotation.nametag_position)?,
                    roster: spans(annotation.roster, annotation.roster_position)?,
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut rows = keyed(rows, MAX_ANNOTATIONS)?;
    rows.retain(|_, annotation| annotation.nametag.is_some() || annotation.roster.is_some());
    Ok(rows)
}

/// Converts and validates a nametag style.
fn nametag_style(style: wit::NametagStyle) -> Result<NametagStyle, String> {
    let style = NametagStyle {
        hide: style.hide,
        hide_players: style.hide_players,
        hide_passengers: style.hide_passengers,
        show_self: style.show_self,
        show_when_hud_hidden: style.show_when_hud_hidden,
        opacity: style.opacity,
        scale: style.scale,
        shadow: style.shadow,
        border_width: style.border_width,
        text_color: style.text_color.map(rgba),
        own_text_color: style.own_text_color.map(rgba),
        border_color: style.border_color.map(rgba),
        background_color: style.background_color.map(rgba),
    };
    style.validate()?;
    Ok(style)
}

impl wit::Host for State {
    fn set_nametag_style(
        &mut self,
        style: Option<wit::NametagStyle>,
    ) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, false)? {
            return Ok(Err(error));
        }
        match style.map(nametag_style).transpose() {
            Ok(style) => {
                self.presentation.stage().nametag_style = style;
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    fn set_annotations(&mut self, rows: Vec<wit::Annotation>) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, !rows.is_empty())? {
            return Ok(Err(error));
        }
        match annotations(rows) {
            Ok(rows) => {
                self.presentation.stage().annotations = rows;
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    fn set_hurt_overlay(
        &mut self,
        body: Option<Rgba>,
        armor: Option<Rgba>,
    ) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, false)? {
            return Ok(Err(error));
        }
        match color(body).and_then(|body| {
            Ok(HurtOverlay {
                body,
                armor: color(armor)?,
            })
        }) {
            Ok(overlay) => {
                self.presentation.stage().hurt_overlay = overlay;
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    fn set_actor_tints(&mut self, tints: Vec<wit::ActorTint>) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, !tints.is_empty())? {
            return Ok(Err(error));
        }
        let rows = tints
            .into_iter()
            .map(|tint| {
                let value = rgba(tint.color);
                validate_color(value).map(|()| (tint.runtime_id, value))
            })
            .collect::<Result<Vec<_>, String>>()
            .and_then(|rows| keyed(rows, MAX_ACTOR_TINTS));
        match rows {
            Ok(rows) => {
                self.presentation.stage().tints = rows;
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(error)),
        }
    }

    fn set_tooltip_lines(&mut self, lines: Vec<wit::TooltipLine>) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, false)? {
            return Ok(Err(error));
        }
        if lines.len() > MAX_TOOLTIP_LINES {
            return Ok(Err("too many tooltip lines".into()));
        }
        let lines: Vec<_> = lines
            .into_iter()
            .map(|line| TooltipLine {
                identifier: line.identifier,
                metadata: line.metadata,
                text: line.text,
                color: rgba(line.color),
            })
            .collect();
        if let Err(error) = lines.iter().try_for_each(TooltipLine::validate) {
            return Ok(Err(error));
        }
        self.presentation.stage().tooltip_lines = lines;
        Ok(Ok(()))
    }

    fn set_chat_actions(&mut self, actions: Vec<wit::ChatAction>) -> Result<Result<(), String>> {
        if let Err(error) = admit(self, false)? {
            return Ok(Err(error));
        }
        if actions.len() > MAX_CHAT_ACTIONS {
            return Ok(Err("too many chat actions".into()));
        }
        let actions: Vec<_> = actions
            .into_iter()
            .map(|action| ChatAction {
                text: action.text,
                action: action.action,
            })
            .collect();
        if let Err(error) = actions.iter().try_for_each(ChatAction::validate) {
            return Ok(Err(error));
        }
        self.presentation.stage().chat_actions = actions;
        Ok(Ok(()))
    }
}

#[cfg(test)]
#[path = "presentation_host_tests.rs"]
mod tests;
