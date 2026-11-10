//! Catalog layering: carrier extensions, the built-in packs under every server pack, and
//! the rules server layers follow, as the screen families register them.
use json_ui::{Catalog, RawControl};

/// Patches a catalog in place.
pub type CatalogPatch = fn(&mut Catalog);

/// Whether a server layer's files author a definition the carrier restores.
pub type AuthoredBy = fn(&Catalog, &[(String, Vec<u8>)]) -> bool;

/// A carrier definition restored before the first server layer that authors it, so that
/// layer patches the carrier's definition rather than a built-in pack's.
#[derive(Clone, Copy)]
pub struct CarrierRestore {
    pub authored: AuthoredBy,
    pub restore: fn(&mut Catalog, &Catalog),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_index_selects_lower_custom_document() {
        let layers = vec![
            vec![(
                "@behub/label.json".into(),
                br#"{"namespace":"custom","label":{"type":"label","text":"Lower file"}}"#.to_vec(),
            )],
            vec![(
                json_ui::UI_DEFS_PATH.into(),
                br#"{"ui_defs":["@behub/label.json"]}"#.to_vec(),
            )],
        ];
        let layered = CatalogPolicy::default().layer(&Catalog::default(), &layers);
        let label = json_ui::resolve(&layered.pack, "custom.label", &json_ui::Context::empty())
            .control
            .unwrap();
        assert_eq!(label.properties["text"], "Lower file");
        assert!(layered.pack.diagnostics().is_empty());
    }

    /// Returns the built-in toast so that layering also builds the screen catalog.
    fn keep_builtin_toast(_: &mut Catalog, builtin: &Catalog) -> Vec<RawControl> {
        builtin
            .lookup("screen", "toast")
            .cloned()
            .into_iter()
            .collect()
    }

    /// The carrier, built-in, pack and screen catalogs stay resident together, so layering a
    /// small server pack copies only the definitions it changes.
    #[test]
    fn layering_shares_definitions_the_server_pack_leaves_unchanged() {
        let controls = (0..400)
            .map(|index| {
                format!(
                    r#""panel_{index}":{{"size":[{index},10],"text":"panel {index}","controls":[{{"icon":{{"layer":{index}}}}}]}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut carrier = Catalog::default();
        let document = format!(r#"{{"namespace":"screen","toast":{{}},{controls}}}"#);
        let ((), parsed) =
            test_alloc::measure(|| carrier.overlay_text("ui/screen.json", &document));
        let mut policy = CatalogPolicy::default();
        policy.after_layers(keep_builtin_toast);
        let layers = vec![vec![
            (
                json_ui::UI_DEFS_PATH.to_owned(),
                br#"{"ui_defs":["ui/screen.json"]}"#.to_vec(),
            ),
            (
                "ui/screen.json".to_owned(),
                br#"{"namespace":"screen","panel_3":{"text":"server"}}"#.to_vec(),
            ),
        ]];
        let (layered, allocated) = test_alloc::measure(|| policy.layer(&carrier, &layers));
        assert!(
            allocated.bytes < parsed.bytes / 2,
            "layering allocated {} bytes over a carrier that parsed in {}",
            allocated.bytes,
            parsed.bytes
        );
        assert_eq!(
            layered.pack.lookup("screen", "panel_3").unwrap().props["text"],
            "server"
        );
        assert_eq!(
            carrier.lookup("screen", "panel_3").unwrap().props["text"],
            "panel 3"
        );
        assert!(
            layered
                .builtin_screens
                .unwrap()
                .lookup("screen", "panel_399")
                .is_some()
        );
    }
}

#[derive(Default)]
pub struct CatalogPolicy {
    extensions: Vec<CatalogPatch>,
    builtin: Vec<CatalogPatch>,
    restores: Vec<CarrierRestore>,
    finishers: Vec<LayerFinisher>,
}

/// Runs after every server layer with the built-in catalog they started from; returns
/// built-in screen controls that gained pack rules.
pub type LayerFinisher = fn(&mut Catalog, &Catalog) -> Vec<RawControl>;

/// A server pack stack over the built-in catalog, and the catalog built-in screens resolve against.
pub struct LayeredCatalogs {
    pub pack: Catalog,
    /// The built-in catalog carrying the pack rules finishers returned; `None` when they returned none.
    pub builtin_screens: Option<Catalog>,
}

impl CatalogPolicy {
    /// Extends the carrier's own catalog, beneath the built-in packs.
    pub fn extend(&mut self, patch: CatalogPatch) {
        self.extensions.push(patch);
    }

    /// Layers a built-in pack over the carrier, beneath every server pack.
    pub fn layer_builtin(&mut self, patch: CatalogPatch) {
        self.builtin.push(patch);
    }

    pub fn restore_before(&mut self, restore: CarrierRestore) {
        self.restores.push(restore);
    }

    /// Runs `finish` after every server layer, with the built-in catalog they started from.
    pub fn after_layers(&mut self, finish: LayerFinisher) {
        self.finishers.push(finish);
    }

    pub(super) fn extended(&self, catalog: &mut Catalog) {
        for patch in &self.extensions {
            patch(catalog);
        }
    }

    /// The carrier under the built-in packs: the catalog with no server pack.
    pub fn builtin(&self, carrier: &Catalog) -> Catalog {
        let mut catalog = carrier.clone();
        for patch in &self.builtin {
            patch(&mut catalog);
        }
        catalog
    }

    /// Server `layers`, bottom first, over the built-in catalog; bootstrap and reload workers
    /// build the same catalog.
    pub fn layer(&self, carrier: &Catalog, layers: &[Vec<(String, Vec<u8>)>]) -> LayeredCatalogs {
        let builtin = self.builtin(carrier);
        let mut catalog = builtin.clone();
        // A higher layer's index may select a document in a lower layer.
        for files in layers {
            for (_, bytes) in files
                .iter()
                .filter(|(path, _)| path == json_ui::UI_DEFS_PATH)
            {
                if let Ok(paths) = Catalog::declared_paths(bytes) {
                    catalog.include_paths(paths);
                }
            }
        }
        let mut restored = vec![false; self.restores.len()];
        for files in layers {
            for (done, rule) in restored.iter_mut().zip(&self.restores) {
                if !*done && (rule.authored)(&catalog, files) {
                    (rule.restore)(&mut catalog, carrier);
                    *done = true;
                }
            }
            catalog.apply_pack(
                files
                    .iter()
                    .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
            );
        }
        let mut screen_controls = Vec::new();
        for finish in &self.finishers {
            screen_controls.extend(finish(&mut catalog, &builtin));
        }
        for note in catalog
            .diagnostics()
            .iter()
            .skip(carrier.diagnostics().len())
        {
            tracing::debug!(note, "server ui pack");
        }
        // Built-in templates retain their geometry. New pack definitions and globals remain
        // available to the message subtrees selected by the screen families.
        let builtin_screens = (!screen_controls.is_empty()).then(|| {
            let mut screens = catalog.clone();
            screens.insert_definitions_from(&builtin);
            for control in screen_controls {
                screens.insert(control);
            }
            screens
        });
        LayeredCatalogs {
            pack: catalog,
            builtin_screens,
        }
    }
}
