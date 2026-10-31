//! Transactional, bounded named-sound suppression.
use super::*;

impl cinnabar::extension::audio::Host for State {
    /// Stages a complete set without altering committed audio on admission failure.
    fn set_muted_sounds(&mut self, mut names: Vec<String>) -> Result<Result<(), String>> {
        self.environment.write()?;
        if !self.grants.audio {
            return Ok(Err("audio capability denied".into()));
        }
        if names.len() > mod_api::MAX_MUTED_SOUNDS
            || names.iter().any(|name| {
                name.is_empty()
                    || name.len() > mod_api::MAX_SOUND_NAME_BYTES
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.:/-".contains(&b))
            })
        {
            return Ok(Err("invalid sound names".into()));
        }
        names.sort_unstable();
        names.dedup();
        self.environment.pending_muted_sounds = Some(names);
        Ok(Ok(()))
    }
}
impl Instance {
    /// Only active, authorized instances can contribute sound suppression.
    pub(crate) fn muted_sounds(&self) -> &[String] {
        if self.is_active() && self.store.data().grants.audio {
            &self.store.data().environment.muted_sounds
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cinnabar::extension::audio::Host as _;

    /// Denial and malformed names never change a valid staged replacement.
    #[test]
    fn sound_suppression_is_granted_bounded_and_transactional() {
        let mut state = State::new(Default::default(), "{}".into(), Default::default());
        assert!(
            state
                .set_muted_sounds(vec!["fixture.one".into()])
                .unwrap()
                .is_err()
        );
        state.grants.audio = true;
        state
            .set_muted_sounds(vec!["fixture.one".into(), "fixture.one".into()])
            .unwrap()
            .unwrap();
        for invalid in [
            vec!["".into()],
            vec!["bad\nname".into()],
            vec!["a".repeat(mod_api::MAX_SOUND_NAME_BYTES + 1)],
            vec!["a".into(); mod_api::MAX_MUTED_SOUNDS + 1],
        ] {
            assert!(state.set_muted_sounds(invalid).unwrap().is_err());
        }
        assert!(state.environment.muted_sounds.is_empty());
        state.environment.commit();
        assert_eq!(state.environment.muted_sounds, ["fixture.one"]);
        state.environment.begin_frame();
        state.environment.begin_callback();
        state.environment.commit();
        assert_eq!(state.environment.muted_sounds, ["fixture.one"]);
        state.set_muted_sounds(vec![]).unwrap().unwrap();
        state.environment.rollback();
        state.environment.commit();
        assert_eq!(state.environment.muted_sounds, ["fixture.one"]);
        state.set_muted_sounds(vec![]).unwrap().unwrap();
        state.environment.commit();
        assert!(state.environment.muted_sounds.is_empty());
        state
            .set_muted_sounds(vec!["fixture.two".into()])
            .unwrap()
            .unwrap();
        state.environment.revoke();
        state.environment.commit();
        assert!(state.environment.muted_sounds.is_empty());
    }
}
