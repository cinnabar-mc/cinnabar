//! Explicit permission declarations for locally packed SDK components.

use anyhow::{Context, Result};
use std::{borrow::Cow, path::Path};
use wasm_encoder::Encode;

/// Packs a guest and validates its optional permission request before writing the result.
pub(super) fn pack(source: &str, output: &str, grants: Option<&str>) -> Result<()> {
    let core = std::fs::read(source)?;
    let mut component = wit_component::ComponentEncoder::default()
        .module(&core)?
        .validate(true)
        .encode()?;
    if let Some(path) = grants {
        let request =
            mod_host::local_files::read(Path::new(path), mod_host::MAX_REQUESTED_GRANTS_BYTES)?;
        add_request(&mut component, &request)?;
    }
    std::fs::write(output, component)?;
    Ok(())
}

/// Adds a top-level request and uses the host's parser to reject malformed or duplicate grants,
/// and the loader to reject a component the new section pushes over its executable byte limit.
/// On error `component` is left unchanged.
fn add_request(component: &mut Vec<u8>, request: &[u8]) -> Result<()> {
    let section = wasm_encoder::CustomSection {
        name: Cow::Borrowed(mod_host::REQUESTED_GRANTS_SECTION),
        data: Cow::Borrowed(request),
    };
    let mut completed = component.clone();
    completed.push(0);
    section.encode(&mut completed);
    let _ = mod_host::declarations_snapshot(&completed)?.requested?;
    mod_host::check_loadable(&completed)
        .context("the component would not load with its requested grants")?;
    *component = completed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_requests_are_visible_and_invalid_or_duplicate_requests_fail() {
        let empty = wat::parse_str("(component)").unwrap();
        let mut component = empty.clone();
        let request = include_bytes!("../../../examples/mods/portal-view/grants.json");
        add_request(&mut component, request).unwrap();
        let grants = mod_host::declarations_snapshot(&component)
            .unwrap()
            .requested
            .unwrap()
            .unwrap();
        assert!(grants.world_draw && grants.render_material);
        assert!(!grants.render_through_walls);
        assert!(add_request(&mut component, request).is_err());
        assert!(add_request(&mut empty.clone(), br#"{"unknown":true}"#).is_err());
    }

    #[test]
    fn a_request_that_would_push_the_component_over_its_limit_is_refused() {
        let mut component = wat::parse_str("(component)").unwrap();
        // Padding brings the component to one byte below the executable limit: the section
        // takes an ID byte, a four-byte length, and the name's length byte and bytes.
        let framing = 1 + 4 + 1 + "pad".len();
        let pad = mod_host::MAX_COMPONENT_BYTES - 1 - component.len() - framing;
        component.push(0);
        wasm_encoder::CustomSection {
            name: Cow::Borrowed("pad"),
            data: Cow::Owned(vec![0; pad]),
        }
        .encode(&mut component);
        assert_eq!(component.len(), mod_host::MAX_COMPONENT_BYTES - 1);
        assert!(mod_host::check_loadable(&component).is_ok());
        let original = component.clone();
        let request = include_bytes!("../../../examples/mods/portal-view/grants.json");
        assert!(add_request(&mut component, request).is_err());
        assert!(
            component == original,
            "a refused request leaves the component unchanged"
        );
    }
}
