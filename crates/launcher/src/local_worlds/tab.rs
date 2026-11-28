//! The categories of the create and edit world screens.

macro_rules! world_tabs {
    ($first:ident: $first_label:literal, $first_supported:literal; $($tab:ident: $label:literal, $supported:literal;)*) => {
        /// Categories shown in the create and edit sidebar.
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub enum Tab { #[default] $first, $($tab,)* }
        impl Tab {
            pub const ALL: &'static [Self] = &[Self::$first, $(Self::$tab,)*];
            /// The sidebar label for this tab.
            pub const fn label(self) -> &'static str {
                match self { Self::$first => $first_label, $(Self::$tab => $label,)* }
            }
            /// Whether this tab has configurable settings.
            pub const fn supported(self) -> bool {
                match self { Self::$first => $first_supported, $(Self::$tab => $supported,)* }
            }
        }
    };
}
world_tabs! {
    General: "General", true;
    Advanced: "Advanced", true;
    Multiplayer: "Multiplayer", true;
    Cheats: "Cheats", true;
    ResourcePacks: "Resource packs", true;
    BehaviourPacks: "Behaviour packs", true;
    Experiments: "Experiments", false;
}

impl Tab {
    /// The pack list this tab edits, if it is a pack tab. Packs live in a saved world's folder,
    /// so these tabs are available only while editing an existing world.
    pub const fn pack_kind(self) -> Option<super::packs::PackKind> {
        match self {
            Self::ResourcePacks => Some(super::packs::PackKind::Resource),
            Self::BehaviourPacks => Some(super::packs::PackKind::Behaviour),
            _ => None,
        }
    }
}
