//! Host-side refusal of exclusive outputs claimed by higher-priority instances.

#[derive(Clone, Default)]
pub(crate) struct Exclusive {
    pub camera: bool,
    /// A consumed one-shot camera publication keeps precedence until the next native frame.
    pub camera_claimed: bool,
    pub environment: bool,
    pub owner: usize,
    pub refused: u64,
}
impl Exclusive {
    /// Records each refused import without trapping the guest.
    pub fn refuse(&mut self) -> String {
        self.refused = self.refused.saturating_add(1);
        "exclusive output belongs to a higher active mod".into()
    }
}
