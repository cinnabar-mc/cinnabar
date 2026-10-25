//! Bounded bindings to facts the host already observes, without guest polling or serialization.

use serde::{Deserialize, Serialize};

/// Selects native rows within the card's explicitly reserved row budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowSource {
    pub kind: NativeRows,
    pub limit: u8,
    #[serde(default = "super::white")]
    pub color: [f32; 4],
    #[serde(default)]
    pub brackets: bool,
}

/// Read-only presentation of the current session's admitted inventory, effects or sidebar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeRows {
    /// Displays native local wall-clock time; no game clock or guest polling is involved.
    Clock {
        military: bool,
        am_pm: bool,
    },
    Inventory {
        hotbar: bool,
    },
    Armor {
        held: bool,
        percent: bool,
        empty: bool,
        /// Uses a held-first equipment stack with unlabelled remaining durability.
        #[serde(default)]
        compact: bool,
        #[serde(default)]
        offhand: bool,
        /// Remaining durability percentage at which values receive warning ink; zero disables it.
        #[serde(default)]
        warning: f32,
    },
    Effects {
        icons: bool,
        /// Separates the localized name and Roman level from a rounded, padded timer.
        #[serde(default)]
        stacked: bool,
        #[serde(default)]
        warnings: bool,
    },
    Sidebar {
        scores: bool,
    },
    /// Counts known inventory and offhand stacks matching these identities or block items.
    ItemCount {
        label: String,
        items: Vec<String>,
        blocks: bool,
        /// Also counts admitted potion forms whose auxiliary type grants this effect.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        potion_effect: Option<String>,
    },
}

impl RowSource {
    /// Reserves the maximum materialized rows before any observations become available.
    pub fn validate(&self) -> Result<(), String> {
        if self.limit == 0 || usize::from(self.limit) > super::MAX_CARD_ROWS {
            return Err("native HUD source requires a bounded positive row limit".into());
        }
        if matches!(self.kind, NativeRows::Clock { .. }) && (self.limit != 1 || self.brackets) {
            return Err(
                "native scalar sources reserve one plain row; text plates own brackets".into(),
            );
        }
        if let NativeRows::ItemCount {
            label,
            items,
            potion_effect,
            ..
        } = &self.kind
        {
            super::text(label)?;
            if items.len() > 16 || items.iter().any(|id| !super::item_identifier(id)) {
                return Err("native item count exceeds its identity bounds".into());
            }
            if potion_effect
                .as_ref()
                .is_some_and(|effect| assets::effect_descriptor_by_identifier(effect).is_none())
            {
                return Err("native item count requires a known potion effect".into());
            }
        }
        if let NativeRows::Armor { warning, .. } = self.kind
            && (!warning.is_finite() || !(0.0..=100.0).contains(&warning))
        {
            return Err("equipment durability warning must be a percentage".into());
        }
        super::color(self.color)
    }
}
