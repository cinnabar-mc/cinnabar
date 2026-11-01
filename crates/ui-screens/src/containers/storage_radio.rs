//! Radio tabs a server pack draws on a storage screen. No screen controller answers them, so
//! the host keeps each group's selected index across rebinds, as vanilla's `ToggleComponent`
//! keeps its own checked state.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use json_ui::{DataSource, HitKind, HitRegion};

use crate::container_actions::Widget;

/// A radio group's key in the screen state, from its `toggle_name`.
fn group_key(name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

/// The group name of a radio toggle region.
fn group_name(region: &HitRegion) -> Option<&str> {
    let toggle = region.widget.toggle.as_ref()?;
    (region.kind == HitKind::Toggle && toggle.radio)
        .then_some(region.control_name.as_deref())
        .flatten()
}

/// The selection a press on `region` makes: its group and place in the group, which a negative
/// forced index reads from the enclosing collection as the toggle component does.
pub(super) fn hit(region: &HitRegion) -> Option<Widget> {
    let name = group_name(region)?;
    let index = region.group_index.or(region.collection_index)?;
    Some(Widget::StorageRadio {
        group: group_key(name),
        index: u16::try_from(index).ok()?,
    })
}

/// Applies the held selections to the groups `hits` drew. A group nobody pressed keeps the
/// toggles' authored default.
pub(super) fn select(data: &mut DataSource, hits: &[HitRegion], selected: &BTreeMap<u64, u16>) {
    if selected.is_empty() {
        return;
    }
    for name in hits.iter().filter_map(group_name) {
        if let Some(index) = selected.get(&group_key(name)) {
            data.select_radio(name, usize::from(*index));
        }
    }
}
