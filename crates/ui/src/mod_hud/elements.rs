//! Vanilla HUD elements a mod may observe and hide, and the HUD root it lays out against.

/// Vanilla HUD elements a mod may read and hide, in the WIT enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VanillaElement {
    Crosshair,
    StatusEffects,
    Sidebar,
    Hunger,
}

impl VanillaElement {
    /// Every element, in enum order.
    pub const ALL: [Self; 4] = [
        Self::Crosshair,
        Self::StatusEffects,
        Self::Sidebar,
        Self::Hunger,
    ];

    /// This element's position in [`Self::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// A set of vanilla elements, such as those mods hide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HiddenElements(u8);

impl HiddenElements {
    /// Collects `elements`, ignoring repeats.
    pub fn from_elements(elements: impl IntoIterator<Item = VanillaElement>) -> Self {
        let mut set = Self::default();
        for element in elements {
            set.0 |= 1 << element.index();
        }
        set
    }

    pub fn contains(self, element: VanillaElement) -> bool {
        self.0 & (1 << element.index()) != 0
    }

    /// Elements hidden by either set, as several mods combine.
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The hidden elements in enum order.
    pub fn iter(self) -> impl Iterator<Item = VanillaElement> {
        VanillaElement::ALL
            .into_iter()
            .filter(move |element| self.contains(*element))
    }
}

/// Whether vanilla draws an element this frame, ignoring hiding, and where.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElementState {
    pub visible: bool,
    /// `[x, y, width, height]` in GUI units from the HUD root's top left, when laid out.
    pub bounds: Option<[f32; 4]>,
}

impl ElementState {
    /// Keeps finite, non-negative extents; anything else reports no bounds.
    pub fn sanitized(self) -> Self {
        Self {
            visible: self.visible,
            bounds: self.bounds.filter(|rect| {
                rect.iter()
                    .all(|value| value.is_finite() && value.abs() <= 1_000_000.)
                    && rect[2] >= 0.
                    && rect[3] >= 0.
            }),
        }
    }
}

/// Every element's state, indexed by [`VanillaElement::index`].
pub type ElementStates = [ElementState; VanillaElement::ALL.len()];

/// The gameplay HUD root in GUI units and the GUI scale, as a HUD surface lays out against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudViewport {
    pub width: f32,
    pub height: f32,
    /// Physical pixels per GUI unit.
    pub scale: f32,
}

impl HudViewport {
    /// Finite positive dimensions and scale.
    pub fn valid(self) -> bool {
        [self.width, self.height, self.scale]
            .iter()
            .all(|value| value.is_finite() && *value > 0. && *value <= 1_000_000.)
    }
}

/// Where an open panel lies over the HUD root, so a mod maps its panel units to the pointer's
/// HUD GUI units: a HUD point `p` lies at `(p - offset) / scale` in panel units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLayout {
    /// The panel root's size in panel units.
    pub size: [f32; 2],
    /// HUD GUI units per panel unit.
    pub scale: f32,
    /// The panel root's top left in HUD GUI units from the HUD root's top left.
    pub offset: [f32; 2],
}

impl PanelLayout {
    /// Finite positive size and scale, and a finite offset.
    pub fn valid(self) -> bool {
        let bounded = |value: f32| value.is_finite() && value.abs() <= 1_000_000.;
        self.size
            .iter()
            .chain([&self.scale])
            .all(|value| bounded(*value) && *value > 0.)
            && self.offset.iter().all(|value| bounded(*value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_layouts_need_finite_positive_sizes_and_scales() {
        let layout = PanelLayout {
            size: [600., 400.],
            scale: 0.5,
            offset: [-2., 3.],
        };
        assert!(layout.valid());
        for invalid in [
            PanelLayout {
                scale: 0.,
                ..layout
            },
            PanelLayout {
                size: [600., f32::INFINITY],
                ..layout
            },
            PanelLayout {
                offset: [f32::NAN, 0.],
                ..layout
            },
        ] {
            assert!(!invalid.valid(), "{invalid:?}");
        }
    }

    #[test]
    fn hidden_elements_combine_as_sets_in_enum_order() {
        let first = HiddenElements::from_elements([
            VanillaElement::Hunger,
            VanillaElement::Crosshair,
            VanillaElement::Hunger,
        ]);
        let second = HiddenElements::from_elements([VanillaElement::Sidebar]);
        let both = first.union(second);
        assert_eq!(
            both.iter().collect::<Vec<_>>(),
            [
                VanillaElement::Crosshair,
                VanillaElement::Sidebar,
                VanillaElement::Hunger
            ]
        );
        assert!(!both.contains(VanillaElement::StatusEffects));
        assert!(HiddenElements::default().is_empty());
    }

    #[test]
    fn element_states_drop_unbounded_rectangles() {
        let state = ElementState {
            visible: true,
            bounds: Some([0., 0., f32::NAN, 1.]),
        };
        assert_eq!(state.sanitized().bounds, None);
        assert!(state.sanitized().visible);
        let state = ElementState {
            visible: false,
            bounds: Some([1., 2., 3., 4.]),
        };
        assert_eq!(state.sanitized(), state);
    }
}
