//! Raw focused input shared with packet reactors through the panel mailbox.
use serde::{Deserialize, Serialize};

/// A capability call denied by the instance's immutable grants.
pub const DENIED: i32 = -1;
/// A capability call rejected without changing retained state.
pub const INVALID: i32 = -2;
/// A panel operation requiring content before it can proceed.
pub const NO_CONTENT: i32 = -3;

/// One control event from the owner's declared panel.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub value: f32,
}
/// One physical press and optional composed text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub key: String,
    pub text: Option<String>,
}
/// Physical input sampled while focused; opening policy belongs to the guest.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Input {
    pub open: bool,
    pub focused: bool,
    pub pointer_held: bool,
    pub pointer_cancelled: bool,
    pub shift_held: bool,
    pub viewport: Option<[f32; 2]>,
    pub held: Vec<String>,
    pub pointer: Option<[f32; 2]>,
    pub wheel: f32,
    pub keys: Vec<Key>,
    pub events: Vec<Event>,
    /// Text changes, submissions and navigation using the shared panel field-event schema.
    pub fields: Vec<serde_json::Value>,
}
