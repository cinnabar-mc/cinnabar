//! Read-only introspection of the current instance's explicit grants.

use super::{State, cinnabar::extension::permissions};
use crate::ModGrants;

impl permissions::Host for State {
    fn read(&mut self) -> anyhow::Result<Vec<permissions::Permission>> {
        self.charge()?;
        Ok(ModGrants::CAPABILITIES
            .iter()
            .map(|capability| permissions::Permission {
                name: capability.name.into(),
                label: capability.label.into(),
                granted: (capability.enabled)(&self.grants),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_every_capability_including_denials_without_granting_authority() {
        let mut state = State::new(
            ModGrants {
                inventory: true,
                ..Default::default()
            },
            String::new(),
            super::super::Declared::default(),
        );
        let permissions = permissions::Host::read(&mut state).unwrap();
        assert_eq!(permissions.len(), ModGrants::CAPABILITIES.len());
        for (permission, capability) in permissions.iter().zip(ModGrants::CAPABILITIES) {
            assert_eq!(permission.name, capability.name);
            assert_eq!(permission.label, capability.label);
            assert_eq!(permission.granted, capability.name == "inventory");
        }
        assert!(!state.grants.camera);
    }
}
