//! Material and cube facts for the 1.7.10 block registry.
//!
//! Each registry row records the block's material, the three class-level answers the game
//! combines with it (whether the block counted as a full block when it was constructed, whether it
//! renders as a normal block, and whether it can provide redstone power) and which collision
//! shape the block's class builds.

use super::shapes::Shape;

/// The 1.7.10 materials that registered blocks use, named after the game's material fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Material {
    Air,
    Grass,
    Ground,
    Wood,
    Rock,
    Iron,
    Anvil,
    Water,
    Lava,
    Leaves,
    Plants,
    Vine,
    Sponge,
    Cloth,
    Fire,
    Sand,
    Circuits,
    Carpet,
    Glass,
    RedstoneLight,
    Tnt,
    Ice,
    PackedIce,
    Snow,
    CraftedSnow,
    Cactus,
    Clay,
    Gourd,
    DragonEgg,
    Portal,
    Cake,
    Web,
    Piston,
}

impl Material {
    /// Whether the material is a liquid: water and lava only.
    #[must_use]
    pub const fn is_liquid(self) -> bool {
        matches!(self, Self::Water | Self::Lava)
    }

    /// Whether the material is solid. The transparent (air, fire), liquid, logic (plants, vine,
    /// circuits, carpet, snow layer) and portal material classes are not.
    #[must_use]
    pub const fn is_solid(self) -> bool {
        !matches!(
            self,
            Self::Air
                | Self::Fire
                | Self::Water
                | Self::Lava
                | Self::Plants
                | Self::Vine
                | Self::Circuits
                | Self::Carpet
                | Self::Snow
                | Self::Portal
        )
    }

    /// Whether the material blocks movement: every solid material except web.
    #[must_use]
    pub const fn blocks_movement(self) -> bool {
        self.is_solid() && !matches!(self, Self::Web)
    }

    /// Whether the material is opaque: it blocks movement and is not flagged translucent.
    #[must_use]
    pub const fn is_opaque(self) -> bool {
        let translucent = matches!(
            self,
            Self::Leaves | Self::Glass | Self::Tnt | Self::Ice | Self::Snow | Self::Cactus
        );
        !translucent && self.blocks_movement()
    }
}

/// What the simulation needs to know about a block id, independent of position and metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockFacts {
    pub material: Material,
    /// The material is a liquid.
    pub liquid: bool,
    /// The material is solid.
    pub solid: bool,
    /// The material blocks movement.
    pub blocks_movement: bool,
    /// The material is opaque.
    pub opaque: bool,
    /// The block is a normal cube: opaque material, renders as a normal block and provides no
    /// redstone power.
    pub normal_cube: bool,
    /// Ground friction factor of the block.
    pub slipperiness: f32,
}

/// A registry row: the public facts, the class-level answers neighbour connections read and the
/// collision shape.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub(crate) facts: BlockFacts,
    /// The block's full-block flag, which the game fixes from its opaque-cube answer while the
    /// block is constructed. Leaves answer "opaque" at that point, and double slabs set the flag
    /// explicitly.
    pub(crate) full_block: bool,
    /// The block renders as a normal block.
    pub(crate) renders_as_cube: bool,
    /// How the block's class builds its collision boxes.
    pub(crate) shape: Shape,
}

impl Entry {
    /// The same row with the friction of ice.
    const fn slippery(mut self) -> Self {
        self.facts.slipperiness = ICE_SLIPPERINESS;
        self
    }
}

/// Friction of every block except ice and packed ice.
const DEFAULT_SLIPPERINESS: f32 = 0.6;
/// Friction of ice and packed ice.
const ICE_SLIPPERINESS: f32 = 0.98;

/// No class-level flag.
const NONE: u8 = 0;
/// The block counted as a full block when constructed.
const FULL: u8 = 1;
/// The block renders as a normal block.
const RENDER: u8 = 2;
/// The block can provide redstone power.
const POWER: u8 = 4;
/// An ordinary cube: full and rendered as a normal block.
const CUBE: u8 = FULL | RENDER;

/// Builds a registry row from a material, its `FULL`/`RENDER`/`POWER` flags and its shape.
const fn entry(material: Material, flags: u8, shape: Shape) -> Entry {
    let renders_as_cube = flags & RENDER != 0;
    Entry {
        facts: BlockFacts {
            material,
            liquid: material.is_liquid(),
            solid: material.is_solid(),
            blocks_movement: material.blocks_movement(),
            opaque: material.is_opaque(),
            normal_cube: material.is_opaque() && renders_as_cube && flags & POWER == 0,
            slipperiness: DEFAULT_SLIPPERINESS,
        },
        full_block: flags & FULL != 0,
        renders_as_cube,
        shape,
    }
}

/// Ids the registry leaves empty resolve to air.
const UNREGISTERED: Entry = entry(Material::Air, RENDER, Shape::None);

/// Number of ids the vanilla registry spans, registered or not.
const REGISTRY_LEN: usize = 176;

/// The registry in numeric id order.
#[rustfmt::skip]
static REGISTRY: [Entry; REGISTRY_LEN] = [
    entry(Material::Air, RENDER, Shape::None), // 0 air
    entry(Material::Rock, CUBE, Shape::FullCube), // 1 stone
    entry(Material::Grass, CUBE, Shape::FullCube), // 2 grass
    entry(Material::Ground, CUBE, Shape::FullCube), // 3 dirt
    entry(Material::Rock, CUBE, Shape::FullCube), // 4 cobblestone
    entry(Material::Wood, CUBE, Shape::FullCube), // 5 planks
    entry(Material::Plants, NONE, Shape::None), // 6 sapling
    entry(Material::Rock, CUBE, Shape::FullCube), // 7 bedrock
    entry(Material::Water, NONE, Shape::None), // 8 flowing_water
    entry(Material::Water, NONE, Shape::None), // 9 water
    entry(Material::Lava, NONE, Shape::None), // 10 flowing_lava
    entry(Material::Lava, NONE, Shape::None), // 11 lava
    entry(Material::Sand, CUBE, Shape::FullCube), // 12 sand
    entry(Material::Sand, CUBE, Shape::FullCube), // 13 gravel
    entry(Material::Rock, CUBE, Shape::FullCube), // 14 gold_ore
    entry(Material::Rock, CUBE, Shape::FullCube), // 15 iron_ore
    entry(Material::Rock, CUBE, Shape::FullCube), // 16 coal_ore
    entry(Material::Wood, CUBE, Shape::FullCube), // 17 log
    entry(Material::Leaves, CUBE, Shape::FullCube), // 18 leaves
    entry(Material::Sponge, CUBE, Shape::FullCube), // 19 sponge
    entry(Material::Glass, NONE, Shape::FullCube), // 20 glass
    entry(Material::Rock, CUBE, Shape::FullCube), // 21 lapis_ore
    entry(Material::Iron, CUBE, Shape::FullCube), // 22 lapis_block
    entry(Material::Rock, CUBE, Shape::FullCube), // 23 dispenser
    entry(Material::Rock, CUBE, Shape::FullCube), // 24 sandstone
    entry(Material::Wood, CUBE, Shape::FullCube), // 25 noteblock
    entry(Material::Cloth, NONE, Shape::Bed), // 26 bed
    entry(Material::Circuits, NONE, Shape::None), // 27 golden_rail
    entry(Material::Circuits, POWER, Shape::None), // 28 detector_rail
    entry(Material::Piston, NONE, Shape::PistonBase), // 29 sticky_piston
    entry(Material::Web, NONE, Shape::None), // 30 web
    entry(Material::Vine, NONE, Shape::None), // 31 tallgrass
    entry(Material::Vine, NONE, Shape::None), // 32 deadbush
    entry(Material::Piston, NONE, Shape::PistonBase), // 33 piston
    entry(Material::Piston, NONE, Shape::PistonHead), // 34 piston_head
    entry(Material::Cloth, CUBE, Shape::FullCube), // 35 wool
    entry(Material::Piston, NONE, Shape::MovingPiston), // 36 piston_extension
    entry(Material::Plants, NONE, Shape::None), // 37 yellow_flower
    entry(Material::Plants, NONE, Shape::None), // 38 red_flower
    entry(Material::Plants, NONE, Shape::None), // 39 brown_mushroom
    entry(Material::Plants, NONE, Shape::None), // 40 red_mushroom
    entry(Material::Iron, CUBE, Shape::FullCube), // 41 gold_block
    entry(Material::Iron, CUBE, Shape::FullCube), // 42 iron_block
    entry(Material::Rock, CUBE, Shape::FullCube), // 43 double_stone_slab
    entry(Material::Rock, NONE, Shape::Slab), // 44 stone_slab
    entry(Material::Rock, CUBE, Shape::FullCube), // 45 brick_block
    entry(Material::Tnt, CUBE, Shape::FullCube), // 46 tnt
    entry(Material::Wood, CUBE, Shape::FullCube), // 47 bookshelf
    entry(Material::Rock, CUBE, Shape::FullCube), // 48 mossy_cobblestone
    entry(Material::Rock, CUBE, Shape::FullCube), // 49 obsidian
    entry(Material::Circuits, NONE, Shape::None), // 50 torch
    entry(Material::Fire, NONE, Shape::None), // 51 fire
    entry(Material::Rock, RENDER, Shape::FullCube), // 52 mob_spawner
    entry(Material::Wood, NONE, Shape::Stairs), // 53 oak_stairs
    entry(Material::Wood, NONE, Shape::Chest), // 54 chest
    entry(Material::Circuits, POWER, Shape::None), // 55 redstone_wire
    entry(Material::Rock, CUBE, Shape::FullCube), // 56 diamond_ore
    entry(Material::Iron, CUBE, Shape::FullCube), // 57 diamond_block
    entry(Material::Wood, CUBE, Shape::FullCube), // 58 crafting_table
    entry(Material::Plants, NONE, Shape::None), // 59 wheat
    entry(Material::Ground, NONE, Shape::Farmland), // 60 farmland
    entry(Material::Rock, CUBE, Shape::FullCube), // 61 furnace
    entry(Material::Rock, CUBE, Shape::FullCube), // 62 lit_furnace
    entry(Material::Wood, NONE, Shape::None), // 63 standing_sign
    entry(Material::Wood, NONE, Shape::Door), // 64 wooden_door
    entry(Material::Circuits, NONE, Shape::Ladder), // 65 ladder
    entry(Material::Circuits, NONE, Shape::None), // 66 rail
    entry(Material::Rock, NONE, Shape::Stairs), // 67 stone_stairs
    entry(Material::Wood, NONE, Shape::None), // 68 wall_sign
    entry(Material::Circuits, POWER, Shape::None), // 69 lever
    entry(Material::Rock, POWER, Shape::None), // 70 stone_pressure_plate
    entry(Material::Iron, NONE, Shape::Door), // 71 iron_door
    entry(Material::Wood, POWER, Shape::None), // 72 wooden_pressure_plate
    entry(Material::Rock, CUBE, Shape::FullCube), // 73 redstone_ore
    entry(Material::Rock, CUBE, Shape::FullCube), // 74 lit_redstone_ore
    entry(Material::Circuits, POWER, Shape::None), // 75 unlit_redstone_torch
    entry(Material::Circuits, POWER, Shape::None), // 76 redstone_torch
    entry(Material::Circuits, POWER, Shape::None), // 77 stone_button
    entry(Material::Snow, NONE, Shape::SnowLayer), // 78 snow_layer
    entry(Material::Ice, RENDER, Shape::FullCube).slippery(), // 79 ice
    entry(Material::CraftedSnow, CUBE, Shape::FullCube), // 80 snow
    entry(Material::Cactus, NONE, Shape::Cactus), // 81 cactus
    entry(Material::Clay, CUBE, Shape::FullCube), // 82 clay
    entry(Material::Plants, NONE, Shape::None), // 83 reeds
    entry(Material::Wood, CUBE, Shape::FullCube), // 84 jukebox
    entry(Material::Wood, NONE, Shape::Fence), // 85 fence
    entry(Material::Gourd, CUBE, Shape::FullCube), // 86 pumpkin
    entry(Material::Rock, CUBE, Shape::FullCube), // 87 netherrack
    entry(Material::Sand, CUBE, Shape::SoulSand), // 88 soul_sand
    entry(Material::Glass, CUBE, Shape::FullCube), // 89 glowstone
    entry(Material::Portal, NONE, Shape::None), // 90 portal
    entry(Material::Gourd, CUBE, Shape::FullCube), // 91 lit_pumpkin
    entry(Material::Cake, NONE, Shape::Cake), // 92 cake
    entry(Material::Circuits, POWER, Shape::Diode), // 93 unpowered_repeater
    entry(Material::Circuits, POWER, Shape::Diode), // 94 powered_repeater
    entry(Material::Glass, NONE, Shape::FullCube), // 95 stained_glass
    entry(Material::Wood, NONE, Shape::Trapdoor), // 96 trapdoor
    entry(Material::Clay, CUBE, Shape::FullCube), // 97 monster_egg
    entry(Material::Rock, CUBE, Shape::FullCube), // 98 stonebrick
    entry(Material::Wood, CUBE, Shape::FullCube), // 99 brown_mushroom_block
    entry(Material::Wood, CUBE, Shape::FullCube), // 100 red_mushroom_block
    entry(Material::Iron, NONE, Shape::Pane), // 101 iron_bars
    entry(Material::Glass, NONE, Shape::Pane), // 102 glass_pane
    entry(Material::Gourd, CUBE, Shape::FullCube), // 103 melon_block
    entry(Material::Plants, NONE, Shape::None), // 104 pumpkin_stem
    entry(Material::Plants, NONE, Shape::None), // 105 melon_stem
    entry(Material::Vine, NONE, Shape::None), // 106 vine
    entry(Material::Wood, NONE, Shape::FenceGate), // 107 fence_gate
    entry(Material::Rock, NONE, Shape::Stairs), // 108 brick_stairs
    entry(Material::Rock, NONE, Shape::Stairs), // 109 stone_brick_stairs
    entry(Material::Grass, CUBE, Shape::FullCube), // 110 mycelium
    entry(Material::Plants, NONE, Shape::LilyPad), // 111 waterlily
    entry(Material::Rock, CUBE, Shape::FullCube), // 112 nether_brick
    entry(Material::Rock, NONE, Shape::Fence), // 113 nether_brick_fence
    entry(Material::Rock, NONE, Shape::Stairs), // 114 nether_brick_stairs
    entry(Material::Plants, NONE, Shape::None), // 115 nether_wart
    entry(Material::Rock, NONE, Shape::EnchantingTable), // 116 enchanting_table
    entry(Material::Iron, NONE, Shape::BrewingStand), // 117 brewing_stand
    entry(Material::Iron, NONE, Shape::Cauldron), // 118 cauldron
    entry(Material::Portal, NONE, Shape::None), // 119 end_portal
    entry(Material::Rock, RENDER, Shape::EndPortalFrame), // 120 end_portal_frame
    entry(Material::Rock, CUBE, Shape::FullCube), // 121 end_stone
    entry(Material::DragonEgg, NONE, Shape::DragonEgg), // 122 dragon_egg
    entry(Material::RedstoneLight, CUBE, Shape::FullCube), // 123 redstone_lamp
    entry(Material::RedstoneLight, CUBE, Shape::FullCube), // 124 lit_redstone_lamp
    entry(Material::Wood, CUBE, Shape::FullCube), // 125 double_wooden_slab
    entry(Material::Wood, NONE, Shape::Slab), // 126 wooden_slab
    entry(Material::Plants, NONE, Shape::Cocoa), // 127 cocoa
    entry(Material::Rock, NONE, Shape::Stairs), // 128 sandstone_stairs
    entry(Material::Rock, CUBE, Shape::FullCube), // 129 emerald_ore
    entry(Material::Rock, NONE, Shape::EnderChest), // 130 ender_chest
    entry(Material::Circuits, POWER, Shape::None), // 131 tripwire_hook
    entry(Material::Circuits, NONE, Shape::None), // 132 tripwire
    entry(Material::Iron, CUBE, Shape::FullCube), // 133 emerald_block
    entry(Material::Wood, NONE, Shape::Stairs), // 134 spruce_stairs
    entry(Material::Wood, NONE, Shape::Stairs), // 135 birch_stairs
    entry(Material::Wood, NONE, Shape::Stairs), // 136 jungle_stairs
    entry(Material::Iron, CUBE, Shape::FullCube), // 137 command_block
    entry(Material::Glass, NONE, Shape::FullCube), // 138 beacon
    entry(Material::Rock, NONE, Shape::Wall), // 139 cobblestone_wall
    entry(Material::Circuits, NONE, Shape::FlowerPot), // 140 flower_pot
    entry(Material::Plants, NONE, Shape::None), // 141 carrots
    entry(Material::Plants, NONE, Shape::None), // 142 potatoes
    entry(Material::Circuits, POWER, Shape::None), // 143 wooden_button
    entry(Material::Circuits, NONE, Shape::Skull), // 144 skull
    entry(Material::Anvil, NONE, Shape::Anvil), // 145 anvil
    entry(Material::Wood, POWER, Shape::Chest), // 146 trapped_chest
    entry(Material::Iron, POWER, Shape::None), // 147 light_weighted_pressure_plate
    entry(Material::Iron, POWER, Shape::None), // 148 heavy_weighted_pressure_plate
    entry(Material::Circuits, POWER, Shape::Diode), // 149 unpowered_comparator
    entry(Material::Circuits, POWER, Shape::Diode), // 150 powered_comparator
    entry(Material::Wood, POWER, Shape::DaylightDetector), // 151 daylight_detector
    entry(Material::Iron, CUBE | POWER, Shape::FullCube), // 152 redstone_block
    entry(Material::Rock, CUBE, Shape::FullCube), // 153 quartz_ore
    entry(Material::Iron, NONE, Shape::Hopper), // 154 hopper
    entry(Material::Rock, CUBE, Shape::FullCube), // 155 quartz_block
    entry(Material::Rock, NONE, Shape::Stairs), // 156 quartz_stairs
    entry(Material::Circuits, NONE, Shape::None), // 157 activator_rail
    entry(Material::Rock, CUBE, Shape::FullCube), // 158 dropper
    entry(Material::Rock, CUBE, Shape::FullCube), // 159 stained_hardened_clay
    entry(Material::Glass, NONE, Shape::Pane), // 160 stained_glass_pane
    entry(Material::Leaves, CUBE, Shape::FullCube), // 161 leaves2
    entry(Material::Wood, CUBE, Shape::FullCube), // 162 log2
    entry(Material::Wood, NONE, Shape::Stairs), // 163 acacia_stairs
    entry(Material::Wood, NONE, Shape::Stairs), // 164 dark_oak_stairs
    UNREGISTERED, // 165 unregistered
    UNREGISTERED, // 166 unregistered
    UNREGISTERED, // 167 unregistered
    UNREGISTERED, // 168 unregistered
    UNREGISTERED, // 169 unregistered
    entry(Material::Grass, CUBE, Shape::FullCube), // 170 hay_block
    entry(Material::Carpet, NONE, Shape::Carpet), // 171 carpet
    entry(Material::Rock, CUBE, Shape::FullCube), // 172 hardened_clay
    entry(Material::Rock, CUBE, Shape::FullCube), // 173 coal_block
    entry(Material::PackedIce, CUBE, Shape::FullCube).slippery(), // 174 packed_ice
    entry(Material::Plants, NONE, Shape::None), // 175 double_plant
];

/// The registry row for a numeric block id; ids the game does not register resolve to air.
#[must_use]
pub(crate) fn entry_of(id: u16) -> &'static Entry {
    REGISTRY.get(usize::from(id)).unwrap_or(&UNREGISTERED)
}

/// Facts for a numeric block id; unknown ids (unregistered or above 175) are air, as the game's
/// registry resolves them.
#[must_use]
pub fn facts(id: u16) -> &'static BlockFacts {
    &entry_of(id).facts
}
