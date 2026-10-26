//! Process-local compiled code shared only while a mod source remains loaded.

use crate::{Source, execution};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use wasmtime::{Engine, component::Component};

pub(crate) struct CompiledComponent {
    pub engine: Engine,
    pub component: Component,
    _ticker: execution::EpochTicker,
    source: Source,
    digest: [u8; 32],
    policy: execution::EnginePolicy,
}

/// Holds no code alive: the last host's unload releases its component, engine and ticker.
fn entries() -> &'static Mutex<BTreeMap<Source, Weak<CompiledComponent>>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<Source, Weak<CompiledComponent>>>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}

impl CompiledComponent {
    /// Reuses code for identical source bytes and engine configuration, never guest state.
    pub fn get(source: &Source, bytes: &[u8], executable: &[u8]) -> Result<Arc<Self>> {
        Self::with_policy(
            source,
            bytes,
            executable,
            execution::EnginePolicy::current(),
        )
    }

    /// Includes every runtime-varying engine option in the source's compilation identity.
    fn with_policy(
        source: &Source,
        bytes: &[u8],
        executable: &[u8],
        policy: execution::EnginePolicy,
    ) -> Result<Arc<Self>> {
        let digest = Sha256::digest(bytes).into();
        let previous = entries()
            .lock()
            .expect("compiled mod cache")
            .get(source)
            .cloned();
        if let Some(previous) = previous.and_then(|entry| entry.upgrade())
            && previous.digest == digest
            && previous.policy == policy
        {
            return Ok(previous);
        }
        let (engine, ticker) = execution::engine_with_policy(policy)?;
        let component = Component::new(&engine, executable)?;
        let compiled = Arc::new(Self {
            engine,
            component,
            _ticker: ticker,
            source: source.clone(),
            digest,
            policy,
        });
        entries()
            .lock()
            .expect("compiled mod cache")
            .insert(source.clone(), Arc::downgrade(&compiled));
        Ok(compiled)
    }
}

impl Drop for CompiledComponent {
    fn drop(&mut self) {
        let mut entries = entries().lock().expect("compiled mod cache");
        if entries
            .get(&self.source)
            .is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self))
        {
            entries.remove(&self.source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_code_but_invalidates_bytes_configuration_and_unload() {
        let directory = tempfile::tempdir().unwrap();
        let source = Source::Component(directory.path().join("component.wat"));
        let bytes = b"(component)";
        let first = CompiledComponent::get(&source, bytes, bytes).unwrap();
        let next = CompiledComponent::get(&source, bytes, bytes).unwrap();
        assert!(Arc::ptr_eq(&first, &next));
        let changed = b"(component (core module))";
        let changed = CompiledComponent::get(&source, changed, changed).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
        let policy = execution::EnginePolicy {
            backtrace: !changed.policy.backtrace,
        };
        let configured = CompiledComponent::with_policy(&source, bytes, bytes, policy).unwrap();
        assert!(!Arc::ptr_eq(&changed, &configured));
        drop(first);
        drop(next);
        drop(changed);
        assert!(entries().lock().unwrap().contains_key(&source));
        let weak = Arc::downgrade(&configured);
        drop(configured);
        assert!(weak.upgrade().is_none());
        assert!(!entries().lock().unwrap().contains_key(&source));
    }
}
