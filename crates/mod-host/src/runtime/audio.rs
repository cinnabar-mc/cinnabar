//! Independent, return-value-only sound cancellation callbacks.
use super::*;

impl Instance {
    /// Only running, authorized components with the export receive sounds.
    pub(crate) fn has_sound_filter(&self) -> bool {
        self.status == InstanceStatus::Running
            && self.store.data().grants.audio
            && self.exports.has_sound_filter()
    }

    /// Bounds observations before lowering; guest failures quarantine without cancelling audio.
    pub(crate) fn filter_sounds(&mut self, events: Vec<crate::SoundEvent>) -> Result<Vec<bool>> {
        self.last_fuel = 0;
        anyhow::ensure!(self.has_sound_filter(), "sound filter authority denied");
        anyhow::ensure!(
            events.len() <= mod_api::MAX_SOUND_FILTER_EVENTS,
            "too many sound events"
        );
        anyhow::ensure!(
            events
                .iter()
                .all(|event| event.name.len() <= mod_api::MAX_SOUND_NAME_BYTES
                    && event
                        .position
                        .as_ref()
                        .is_none_or(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())),
            "invalid sound observation"
        );
        let count = events.len();
        crate::execution::arm(&mut self.store, FRAME_FUEL, false)?;
        self.store.call_hook(|_, hook| {
            anyhow::ensure!(
                !matches!(hook, wasmtime::CallHook::CallingHost),
                "sound filters cannot call host imports"
            );
            Ok(())
        });
        let result = self
            .exports
            .filter_sounds(&mut self.store, events)
            .and_then(|verdict| {
                anyhow::ensure!(
                    verdict.len() == count,
                    "sound verdict length differs from observations"
                );
                Ok(verdict)
            });
        self.store.call_hook(|_, _| Ok(()));
        self.last_fuel = FRAME_FUEL - self.store.get_fuel().unwrap_or(0).min(FRAME_FUEL);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
}

impl cinnabar::extension::sound_filter::Host for State {}
