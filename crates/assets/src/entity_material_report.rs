//! Counted entity material behaviour of one compiled server pack, reported once per pack
//! generation. Entity compilation fills these counts; reports and diagnostics read them.

/// Counted list entries the client loads nothing from or reads only in part.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, serde::Serialize)]
pub struct MaterialListReport {
    /// Entries carrying `tag`, which Bedrock logs as no longer supported and ignores.
    pub ignored_tags: u32,
    /// Entries whose `path` is absent, empty, not a string or too long to copy.
    pub empty_paths: u32,
    /// Entries naming a file an earlier entry already loaded; the earlier entry's defines win.
    pub repeated_paths: u32,
    /// List copies that are not JSON.
    pub unparsable_lists: u32,
}

impl MaterialListReport {
    /// Adds `other`'s counts to these.
    pub fn add(&mut self, other: Self) {
        self.ignored_tags += other.ignored_tags;
        self.empty_paths += other.empty_paths;
        self.repeated_paths += other.repeated_paths;
        self.unparsable_lists += other.unparsable_lists;
    }
}

/// One compiled pack's entity material counts.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, serde::Serialize)]
pub struct EntityMaterialDiagnostics {
    /// Distinct state names no Bedrock state matches. Bedrock asserts on them; the client
    /// ignores them.
    pub unknown_states: u32,
    /// Distinct define names Bedrock gives no actor flag. It ignores them without a log, and so
    /// does the client.
    pub unknown_defines: u32,
    /// `samplerStates` entries whose `samplerIndex` is 8 or more, skipped as Bedrock skips them.
    pub skipped_samplers: u32,
    /// Declared materials that do not resolve: a parent missing from their file, a parent
    /// cycle, or a malformed state or define list.
    pub failed_materials: u32,
    /// Resolved materials whose program has no actor material type; they draw as plain `Actor`.
    pub unassignable_programs: u32,
    /// Resolved materials with stencil, polygon-offset or depth-bias state the client carries
    /// without rendering.
    pub unsupported_materials: u32,
    /// Resolved materials drawn approximately: actor defines the client carries without
    /// drawing, or the `pattern` type drawn as `entity`.
    pub approximated_materials: u32,
    /// Material draws dropped because their render controller supplies too few textures.
    pub skipped_draws: u32,
    /// `materials/*.material` files no list entry named, loaded in place of the vanilla lists.
    pub unlisted_files: u32,
    /// List entries the client read in part or loaded nothing from.
    pub list: MaterialListReport,
}

impl EntityMaterialDiagnostics {
    /// Whether any count is non-zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
