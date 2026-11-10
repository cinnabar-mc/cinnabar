//! Granted, import-free event handlers.
use crate::{ModHost, SoundContext, TeleportContext};
impl ModHost {
    /// Whether this running component may cancel sound playback requests.
    pub fn has_sound_handler(&self) -> bool {
        self.instance.has_sound_handler()
    }
    /// Whether this running component may replace local-player MovePlayer rotations.
    pub fn has_teleport_handler(&self) -> bool {
        self.instance.has_teleport_handler()
    }
    /// The sound after this handler; failures leave the incoming context to the caller.
    pub fn handle_sound(&mut self, ctx: SoundContext) -> anyhow::Result<SoundContext> {
        self.instance.handle_sound(ctx)
    }
    /// The move after this handler; failures leave the incoming context to the caller.
    pub fn handle_teleport(&mut self, ctx: TeleportContext) -> anyhow::Result<TeleportContext> {
        self.instance.handle_teleport(ctx)
    }
}
