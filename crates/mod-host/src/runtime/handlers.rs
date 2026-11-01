//! Import-free event handlers: each call receives a context and returns it changed.
use super::*;
use crate::{SoundContext, TeleportContext, TeleportRotation};

impl Instance {
    /// Only running, audio-authorized components exporting `handle-sound` receive sounds.
    pub(crate) fn has_sound_handler(&self) -> bool {
        self.status == InstanceStatus::Running
            && self.store.data().grants.audio
            && self.exports.has_sound_handler()
    }

    /// Only running, camera-authorized components exporting `handle-teleport` receive moves.
    pub(crate) fn has_teleport_handler(&self) -> bool {
        self.status == InstanceStatus::Running
            && self.store.data().grants.camera
            && self.exports.has_teleport_handler()
    }

    /// Returns the incoming sound with only `cancelled` taken from the handler; once
    /// cancelled, a sound stays cancelled.
    pub(crate) fn handle_sound(&mut self, ctx: SoundContext) -> Result<SoundContext> {
        anyhow::ensure!(self.has_sound_handler(), "sound handler authority denied");
        anyhow::ensure!(
            ctx.name.len() <= mod_api::MAX_SOUND_NAME_BYTES
                && ctx
                    .position
                    .as_ref()
                    .is_none_or(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()),
            "invalid sound context"
        );
        let returned =
            self.run_handler(|exports, store| exports.handle_sound(store, ctx.clone()))?;
        Ok(SoundContext {
            cancelled: ctx.cancelled || returned.cancelled,
            ..ctx
        })
    }

    /// Returns the incoming move with only `rotation` taken from the handler; a replacement
    /// must be finite with pitch within 90 degrees, while the incoming rotation always passes.
    pub(crate) fn handle_teleport(&mut self, ctx: TeleportContext) -> Result<TeleportContext> {
        anyhow::ensure!(
            self.has_teleport_handler(),
            "teleport handler authority denied"
        );
        let returned = self.run_handler(|exports, store| exports.handle_teleport(store, ctx))?;
        // The server's rotation may lie outside the replacement bounds; passing it through
        // unchanged is never the component's fault.
        if returned.rotation != ctx.rotation && !valid_rotation(&returned.rotation) {
            self.quarantine();
            anyhow::bail!("teleport rotation is not finite or its pitch exceeds 90 degrees");
        }
        Ok(TeleportContext {
            rotation: returned.rotation,
            ..ctx
        })
    }

    /// Runs one handler call within `HANDLER_CALL_FUEL`, refusing host imports; a trap
    /// quarantines the component.
    fn run_handler<R>(
        &mut self,
        call: impl FnOnce(&exports::Exports, &mut Store<State>) -> Result<R>,
    ) -> Result<R> {
        self.last_fuel = 0;
        let fuel = mod_api::HANDLER_CALL_FUEL;
        crate::execution::arm(&mut self.store, fuel, false)?;
        self.store.call_hook(|_, hook| {
            anyhow::ensure!(
                !matches!(hook, wasmtime::CallHook::CallingHost),
                "event handlers cannot call host imports"
            );
            Ok(())
        });
        let result = call(&self.exports, &mut self.store);
        self.store.call_hook(|_, _| Ok(()));
        self.last_fuel = fuel - self.store.get_fuel().unwrap_or(0).min(fuel);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
}

/// A finite Bedrock rotation whose pitch stays within straight up and straight down.
fn valid_rotation(rotation: &TeleportRotation) -> bool {
    rotation.yaw.is_finite() && rotation.pitch.is_finite() && rotation.pitch.abs() <= 90.0
}

impl cinnabar::extension::sound_handling::Host for State {}
impl cinnabar::extension::teleport_handling::Host for State {}
