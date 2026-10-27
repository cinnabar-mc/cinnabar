//! Block `render_method` names from `minecraft:material_instances`.

/// A block material's render layer, in Bedrock's declaration order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum BlockRenderMethod {
    DoubleSided,
    Blend,
    BlendToOpaque,
    #[default]
    Opaque,
    AlphaTestToOpaque,
    AlphaTest,
    AlphaTestSingleSided,
    AlphaTestSingleSidedToOpaque,
}

impl BlockRenderMethod {
    /// Every render method in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::DoubleSided,
        Self::Blend,
        Self::BlendToOpaque,
        Self::Opaque,
        Self::AlphaTestToOpaque,
        Self::AlphaTest,
        Self::AlphaTestSingleSided,
        Self::AlphaTestSingleSidedToOpaque,
    ];

    /// The authored and network spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::DoubleSided => "double_sided",
            Self::Blend => "blend",
            Self::BlendToOpaque => "blend_to_opaque",
            Self::Opaque => "opaque",
            Self::AlphaTestToOpaque => "alpha_test_to_opaque",
            Self::AlphaTest => "alpha_test",
            Self::AlphaTestSingleSided => "alpha_test_single_sided",
            Self::AlphaTestSingleSidedToOpaque => "alpha_test_single_sided_to_opaque",
        }
    }

    /// The native block render layer value this method selects.
    pub const fn layer(self) -> u8 {
        match self {
            Self::DoubleSided => 0,
            Self::Blend => 3,
            Self::BlendToOpaque => 4,
            Self::Opaque => 5,
            Self::AlphaTestToOpaque => 7,
            Self::AlphaTest => 8,
            Self::AlphaTestSingleSided => 11,
            Self::AlphaTestSingleSidedToOpaque => 12,
        }
    }

    /// Resolves an exact spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|method| method.name() == name)
    }

    /// Resolves a network value as the client does: a missing or unknown name is opaque.
    pub fn from_network(name: Option<&str>) -> Self {
        name.and_then(Self::from_name).unwrap_or_default()
    }

    /// Whether the material blends with what is behind it.
    pub const fn blends(self) -> bool {
        matches!(self, Self::Blend | Self::BlendToOpaque)
    }

    /// Whether the material discards texels below the alpha threshold.
    pub const fn alpha_tests(self) -> bool {
        matches!(
            self,
            Self::AlphaTest
                | Self::AlphaTestToOpaque
                | Self::AlphaTestSingleSided
                | Self::AlphaTestSingleSidedToOpaque
        )
    }

    /// Whether back faces draw.
    pub const fn two_sided(self) -> bool {
        matches!(
            self,
            Self::DoubleSided
                | Self::Blend
                | Self::BlendToOpaque
                | Self::AlphaTest
                | Self::AlphaTestToOpaque
        )
    }

    /// Whether distant geometry draws through the opaque layer instead of this method.
    pub const fn turns_opaque_in_distance(self) -> bool {
        matches!(
            self,
            Self::BlendToOpaque | Self::AlphaTestToOpaque | Self::AlphaTestSingleSidedToOpaque
        )
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::BlockRenderMethod;

    #[test]
    fn missing_and_unknown_network_names_are_opaque() {
        assert_eq!(
            BlockRenderMethod::from_network(None),
            BlockRenderMethod::Opaque
        );
        assert_eq!(
            BlockRenderMethod::from_network(Some("future_method")),
            BlockRenderMethod::Opaque
        );
        assert_eq!(
            BlockRenderMethod::from_network(Some("alpha_test")),
            BlockRenderMethod::AlphaTest
        );
    }

    /// The evidence matrix lists each name with its native layer value.
    #[test]
    fn names_and_layers_match_the_evidence_matrix() {
        let matrix = include_str!("../../../docs/evidence/material-behavior.tsv");
        let row = matrix
            .lines()
            .find(|line| line.starts_with("block.render_method\t"))
            .expect("render_method evidence row");
        let conditions = row.split('\t').nth(11).expect("conditions column");
        let mut documented = conditions
            .split_whitespace()
            .map(|pair| {
                let (name, layer) = pair.split_once('=').expect("name=layer");
                (name, layer.parse::<u8>().expect("layer value"))
            })
            .collect::<std::vec::Vec<_>>();
        let mut declared = BlockRenderMethod::ALL
            .iter()
            .map(|method| (method.name(), method.layer()))
            .collect::<std::vec::Vec<_>>();
        documented.sort_unstable();
        declared.sort_unstable();
        assert_eq!(declared, documented);
    }
}
