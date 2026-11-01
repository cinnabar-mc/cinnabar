//! A radio toggle's place in its group and the state the screen's selection gives it.

use serde_json::Value;

use super::native::{self, Native};
use super::{Bag, DataSource, Scope};
use crate::predicate::Scalar;
use crate::tree::ResolvedControl;

/// The baked place of a radio toggle in its group.
pub(crate) const RADIO_INDEX_KEY: &str = "#radio_index";

/// A radio-group toggle is checked when the screen selected its index, whichever bindings
/// answered its state. Its index is baked for input, so a press selects what this compares.
pub(super) fn apply(
    data: &DataSource,
    control: &ResolvedControl,
    scope: &Scope,
    own: &mut Bag,
    native: &mut Native,
) {
    let Some(index) = radio_index(control, scope, own, native) else {
        own.remove(RADIO_INDEX_KEY);
        return;
    };
    own.insert(RADIO_INDEX_KEY.to_owned(), Scalar::Int(index));
    let Some(selected) = control
        .properties
        .get("toggle_name")
        .and_then(Value::as_str)
        .and_then(|name| data.globals.get(&format!("#radio:{name}")))
        .and_then(Scalar::as_number)
    else {
        return;
    };
    let checked = selected == index as f64;
    own.insert("#toggle_state".to_owned(), Scalar::Bool(checked));
    native::apply("#toggle_state", &Value::Bool(checked), control, own, native);
}

/// A radio toggle's place in its group, as `ToggleComponent::_setChecked` reads it: a non-negative
/// forced index, whose `#name` form reads the toggle's own bound value (collection rows), else
/// its item in the collection it names.
fn radio_index(
    control: &ResolvedControl,
    scope: &Scope,
    own: &Bag,
    native: &Native,
) -> Option<i64> {
    let forced = native
        .props
        .get("toggle_group_forced_index")
        .or_else(|| control.properties.get("toggle_group_forced_index"));
    forced
        .and_then(Value::as_f64)
        .or_else(|| match own.get(forced?.as_str()?)? {
            Scalar::Text(text) => text.parse().ok(),
            other => other.as_number(),
        })
        .filter(|index| *index >= 0.0)
        .map(|index| index as i64)
        .or_else(|| {
            let name = control
                .properties
                .get("toggle_grid_collection_name")?
                .as_str()?;
            scope.cursor.indices.get(name).copied()
        })
        .filter(|index| *index >= 0)
}
