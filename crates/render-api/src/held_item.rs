//! Held placement facts for server-defined items, shared by the registry parser and renderer.

/// One hand and perspective's placement: blocks, degrees and per-texel scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeldItemTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

/// A hand's placements in each perspective.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeldItemHand {
    pub first_person: HeldItemTransform,
    pub third_person: HeldItemTransform,
}

/// The tables of a `minecraft:render_offsets` component. Values are always finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemRenderOffsets {
    pub main_hand: HeldItemHand,
    pub off_hand: HeldItemHand,
}

// Registry facts compare by value, and the parser admits only finite numbers.
impl Eq for ItemRenderOffsets {}

const DEFAULT_THIRD_PERSON: HeldItemTransform = HeldItemTransform {
    position: [0.258, 0.979, -0.541],
    rotation: [-63.268, -43.969, 144.041],
    scale: [0.094, 0.094, 0.094],
};

/// Vanilla's built-in tables, used for an item with no component and as the base that a
/// component's declared fields overwrite.
pub const DEFAULT_ITEM_RENDER_OFFSETS: ItemRenderOffsets = ItemRenderOffsets {
    main_hand: HeldItemHand {
        first_person: HeldItemTransform {
            position: [0.48, -0.128, -0.946],
            rotation: [11.696, -64.536, 79.413],
            scale: [0.038, 0.037, 0.038],
        },
        third_person: DEFAULT_THIRD_PERSON,
    },
    off_hand: HeldItemHand {
        first_person: HeldItemTransform {
            position: [-1.053, 0.136, -0.803],
            rotation: [27.273, 67.731, -64.494],
            scale: [0.063, 0.063, 0.062],
        },
        third_person: DEFAULT_THIRD_PERSON,
    },
};

/// How an item's flat sprite is placed in the hand.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum HeldItemPlacement {
    /// Retail items keep the existing retail sprite transforms.
    #[default]
    Retail,
    /// A server-defined item with no component: the default tables, scaled so any icon size
    /// spans the same length.
    DefaultOffsets,
    /// A declared component: its tables, with no icon size or actor scale factor.
    DeclaredOffsets(ItemRenderOffsets),
}

/// A hand a held sprite is placed in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldItemHandSide {
    Main,
    Off,
}

/// The perspective a held sprite is drawn from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldItemPerspective {
    FirstPerson,
    ThirdPerson,
}

impl ItemRenderOffsets {
    /// Selects one hand and perspective's placement.
    pub const fn transform(
        &self,
        hand: HeldItemHandSide,
        perspective: HeldItemPerspective,
    ) -> &HeldItemTransform {
        let hand = match hand {
            HeldItemHandSide::Main => &self.main_hand,
            HeldItemHandSide::Off => &self.off_hand,
        };
        match perspective {
            HeldItemPerspective::FirstPerson => &hand.first_person,
            HeldItemPerspective::ThirdPerson => &hand.third_person,
        }
    }
}
