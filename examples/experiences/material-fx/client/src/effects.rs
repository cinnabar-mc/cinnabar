//! The showcase's materials: each one's shader (the shared prelude, then its own source), the
//! textures it declares, its host blend class, its target, the richer form it takes when the
//! host grants world material effects, and the parameters computed from the server-driven
//! [`State`].

use crate::operations;
use experience_sdk::client::world_material::{Blend, Depth, MaterialSpec, Target};
use experience_sdk::client::world_material_effects as wme;

/// One showcase material.
pub struct Effect {
    pub source: &'static str,
    pub textures: &'static [&'static str],
    pub blend: Blend,
    /// What it restyles; `None` for a material other bindings switch to.
    pub target: fn() -> Option<Target>,
    /// Its form with world material effects, which needs their permissions; when the host
    /// refuses it, the plain form above is declared instead.
    pub rich: Option<Rich>,
}

/// A material's form with world material effects: its source (the plain one when `None`) and
/// the effects it asks for under the current state.
pub struct Rich {
    pub source: Option<&'static str>,
    pub effects: fn(&State) -> wme::Effects,
}

/// A shader with the shared prelude.
macro_rules! shader {
    ($name:literal) => {
        concat!(
            include_str!("../shaders/common.wgsl"),
            include_str!(concat!("../shaders/", $name, ".wgsl"))
        )
    };
}

/// A material for the Experience's entity `name`.
macro_rules! mob {
    ($name:literal) => {
        || Some(Target::Entity(concat!("materialfx:", $name).into()))
    };
}

/// A material for the Experience's block `name`.
macro_rules! block {
    ($name:literal) => {
        || Some(Target::Block(concat!("materialfx:", $name).into()))
    };
}

/// Material and binding ids, which index [`EFFECTS`].
pub const WRAITH: u32 = 0;
pub const HOLOGRAM: u32 = 1;
pub const VOID_WALKER: u32 = 2;
pub const LENTICULAR: u32 = 3;
pub const METAL: u32 = 4;
pub const THERMAL: u32 = 5;
pub const BOTTLE: u32 = 6;
pub const AQUARIUM: u32 = 7;
pub const MINESHAFT: u32 = 8;
pub const ARENA: u32 = 9;
pub const BLACK_HOLE: u32 = 10;
pub const PAINTING: u32 = 11;
pub const DIAMOND: u32 = 12;
pub const REDSTONE: u32 = 13;
pub const CORRUPTION: u32 = 14;
pub const SNOW_GLOBE: u32 = 15;
pub const LAVA_LAMP: u32 = 16;
pub const SCREEN: u32 = 17;
pub const RUNE: u32 = 18;
pub const PORTAL: u32 = 19;
pub const WAVE_GRASS: u32 = 20;
pub const FLAG: u32 = 21;
pub const JELLY: u32 = 22;
pub const SHATTER: u32 = 23;
pub const LENS: u32 = 24;
pub const HAZE: u32 = 25;
pub const FORCE_FIELD: u32 = 26;
pub const BOSS: u32 = 27;
pub const STORM: u32 = 28;
pub const FIRE: u32 = 29;

/// The mobs thermal vision recolours.
pub const MOBS: [u32; 5] = [WRAITH, HOLOGRAM, VOID_WALKER, LENTICULAR, METAL];

/// The showcase's materials, by id: the gallery's, then the effects wing's.
pub const EFFECTS: [Effect; 30] = [
    Effect {
        source: shader!("dissolve"),
        textures: &["textures/noise.png"],
        blend: Blend::Cutout,
        target: mob!("wraith"),
        rich: None,
    },
    Effect {
        source: shader!("hologram"),
        textures: &[],
        blend: Blend::Additive,
        target: mob!("hologram"),
        rich: None,
    },
    Effect {
        source: shader!("voidwalker"),
        textures: &[],
        blend: Blend::Cutout,
        target: mob!("voidwalker"),
        rich: None,
    },
    Effect {
        source: shader!("lenticular"),
        textures: &["textures/lenticular.png"],
        blend: Blend::Cutout,
        target: mob!("lenticular"),
        rich: None,
    },
    Effect {
        source: shader!("metal"),
        textures: &["textures/skybox.png"],
        blend: Blend::Cutout,
        target: mob!("metal"),
        rich: None,
    },
    Effect {
        source: shader!("thermal"),
        textures: &[],
        blend: Blend::Cutout,
        target: || None,
        rich: None,
    },
    Effect {
        source: shader!("bottle"),
        textures: &["textures/diorama.png"],
        blend: Blend::Premultiplied,
        target: block!("bottle"),
        rich: None,
    },
    Effect {
        source: shader!("hollow"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("aquarium"),
        rich: None,
    },
    Effect {
        source: shader!("hollow"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("mineshaft"),
        rich: None,
    },
    Effect {
        source: shader!("arena"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("arena"),
        rich: None,
    },
    Effect {
        source: shader!("blackhole"),
        textures: &[],
        blend: Blend::Premultiplied,
        target: block!("blackhole"),
        rich: Some(Rich {
            source: Some(shader!("blackhole_lens")),
            effects: scene_effects,
        }),
    },
    Effect {
        source: shader!("canvas"),
        textures: &["textures/painting.png"],
        blend: Blend::Opaque,
        target: block!("canvas"),
        rich: None,
    },
    Effect {
        source: shader!("ore"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("glintore"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("ore"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("pulseore"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("corruption"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("wallstone"),
        rich: None,
    },
    Effect {
        source: shader!("snowglobe"),
        textures: &[],
        blend: Blend::Premultiplied,
        target: block!("snowglobe"),
        rich: None,
    },
    Effect {
        source: shader!("lavalamp"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("lavalamp"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("screen"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("screen"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("rune"),
        textures: &["textures/rune.png"],
        blend: Blend::Opaque,
        target: block!("rune"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("vortex"),
        textures: &["textures/stars.png"],
        blend: Blend::Opaque,
        target: block!("vortex"),
        rich: Some(Rich {
            source: None,
            effects: bloom_effects,
        }),
    },
    Effect {
        source: shader!("wavegrass"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("wavegrass"),
        rich: Some(Rich {
            source: None,
            effects: |_| moving(0.25),
        }),
    },
    Effect {
        source: shader!("flag"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("flag"),
        rich: Some(Rich {
            source: None,
            effects: |_| moving(0.5),
        }),
    },
    Effect {
        source: shader!("jelly"),
        textures: &[],
        blend: Blend::Premultiplied,
        target: mob!("jelly"),
        rich: Some(Rich {
            source: None,
            effects: |_| moving(0.5),
        }),
    },
    Effect {
        source: shader!("shatter"),
        textures: &[],
        blend: Blend::Cutout,
        target: mob!("shatter"),
        rich: Some(Rich {
            source: None,
            effects: |_| moving(2.0),
        }),
    },
    Effect {
        source: shader!("lens"),
        textures: &[],
        blend: Blend::Premultiplied,
        target: block!("lens"),
        rich: Some(Rich {
            source: None,
            effects: scene_effects,
        }),
    },
    Effect {
        source: shader!("haze"),
        textures: &[],
        blend: Blend::Premultiplied,
        target: block!("haze"),
        rich: Some(Rich {
            source: None,
            effects: scene_effects,
        }),
    },
    Effect {
        source: shader!("forcefield"),
        textures: &[],
        blend: Blend::Additive,
        target: block!("forcefield"),
        rich: Some(Rich {
            source: None,
            effects: scene_effects,
        }),
    },
    Effect {
        source: shader!("boss"),
        textures: &[],
        blend: Blend::Opaque,
        target: mob!("boss"),
        rich: Some(Rich {
            source: None,
            effects: |state| wme::Effects {
                outline: state.outline.then_some(wme::Outline { width_pixels: 2.0 }),
                ..bloom_effects(state)
            },
        }),
    },
    Effect {
        source: shader!("storm"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("storm"),
        rich: Some(Rich {
            source: None,
            effects: |state| wme::Effects {
                particles: Some(wme::Particles {
                    count: STORM_CELLS,
                    radius_blocks: 4.0,
                }),
                cells: Some(STORM_COMPUTE),
                ..bloom_effects(state)
            },
        }),
    },
    Effect {
        source: shader!("firewall"),
        textures: &[],
        blend: Blend::Opaque,
        target: block!("firewall"),
        rich: Some(Rich {
            source: None,
            effects: |state| wme::Effects {
                cells: Some(FIRE_COMPUTE),
                ..bloom_effects(state)
            },
        }),
    },
];

/// No effects: what a rich form starts from.
pub const NO_EFFECTS: wme::Effects = wme::Effects {
    displacement_blocks: None,
    scene: false,
    outline: None,
    bloom: None,
    particles: None,
    cells: None,
};

/// Vertex displacement within `blocks`.
fn moving(blocks: f32) -> wme::Effects {
    wme::Effects {
        displacement_blocks: Some(blocks),
        ..NO_EFFECTS
    }
}

/// Reading the scene.
fn scene_effects(_: &State) -> wme::Effects {
    wme::Effects {
        scene: true,
        ..NO_EFFECTS
    }
}

/// A bloom of the emission, while bloom is on.
fn bloom_effects(state: &State) -> wme::Effects {
    wme::Effects {
        bloom: state.bloom.then_some(wme::Bloom {
            strength: 1.0,
            radius_pixels: 3.0,
        }),
        ..NO_EFFECTS
    }
}

/// The storm's compute id and its cells, one particle each.
pub const STORM_COMPUTE: u32 = 0;
pub const STORM_CELLS: u32 = 1024;
/// The fire's compute id, and its wall in texels: four blocks by three.
pub const FIRE_COMPUTE: u32 = 1;
pub const FIRE_WIDTH: u32 = 64;
pub const FIRE_HEIGHT: u32 = 48;

/// The showcase's compute declarations: their source, cells and parameters under `state`.
pub fn compute(id: u32, state: &State) -> wme::ComputeSpec {
    let (source, cells, params) = match id {
        STORM_COMPUTE => (
            include_str!("../shaders/storm_step.wgsl"),
            STORM_CELLS,
            vec![state.storm, 60.0],
        ),
        _ => (
            include_str!("../shaders/fire_step.wgsl"),
            FIRE_WIDTH * FIRE_HEIGHT,
            vec![state.fire.unwrap_or(-1.0), FIRE_WIDTH as f32, 7.0],
        ),
    };
    wme::ComputeSpec {
        source: source.into(),
        cells,
        params,
    }
}

/// Every compute id the showcase declares.
pub const COMPUTES: [u32; 2] = [STORM_COMPUTE, FIRE_COMPUTE];

/// Thousandths per block, the unit the server sends positions and times in.
const MILLI: f32 = 1000.0;

/// An animation phase `seconds * rate + offset` on the session clock, as the shaders compute it, whose rate the server
/// changes. Changing the rate at a moment moves the offset so the phase carries on from where it
/// was, instead of jumping to where the new rate would have put it since the clock began.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Phase {
    /// Phase units per session second.
    pub rate: f32,
    /// The phase at session second zero.
    pub offset: f64,
}

impl Phase {
    /// A phase advancing at `rate` that is zero at session second `at`.
    pub fn starting(at: f64, rate: f32) -> Self {
        Self {
            rate,
            offset: -at * f64::from(rate),
        }
    }

    /// Changes the rate to `rate` at session second `at`, keeping the phase there.
    pub fn retime(&mut self, at: f64, rate: f32) {
        self.offset += at * (f64::from(self.rate) - f64::from(rate));
        self.rate = rate;
    }
}

/// The portal's closeness, 1 at its face and 0 from twelve blocks away, for a player
/// `distance` blocks from it.
pub fn portal_near(distance: f32) -> f32 {
    (1.0 - distance / 12.0).clamp(0.0, 1.0)
}

/// How fast the portal turns, in phase units per second, at closeness `near`.
pub fn portal_rate(near: f32) -> f32 {
    0.4 + 1.6 * near
}

/// Heartbeats per second at boss health `health` (0 to 1): faster as it falls.
pub fn beat_rate(health: f32) -> f32 {
    1.0 / (0.45 + (1.4 - 0.45) * health.clamp(0.0, 1.0))
}

/// What the server has said so far.
pub struct State {
    /// Materials declared so far, one bit per id; the host keeps them across callbacks.
    pub declared: u64,
    /// Materials declared in their rich form, one bit per id.
    pub rich: u64,
    /// Computes declared so far, one bit per id.
    pub computes: u32,
    pub thermal: bool,
    /// The held wraith cut in thousandths, or `None` to sweep.
    cut: Option<i64>,
    /// The last shockwave: x, z and start (seconds).
    boom: [f32; 3],
    /// The heartbeat's phase in beats, if it beats.
    heartbeat: Option<Phase>,
    /// The last liquid metal hit: x, y, z and start (seconds).
    hit: [f32; 4],
    /// The corruption's source and start, if it spreads.
    corruption: Option<[f32; 4]>,
    /// The centres of the arena, black hole, painting, screen and portal, then the flag's
    /// cloth, the fire wall, the force field and the top of the grass field.
    centres: [[f32; 3]; 9],
    /// The layout's facing in quarter turns from south.
    quarter: f32,
    /// Up to four players' x and z, and how many there are.
    players: [[f32; 2]; 4],
    player_count: f32,
    /// The player's closeness to the portal ([`portal_near`]) and its turning phase.
    portal_near: f32,
    portal_spin: Phase,
    /// The wind over the grass and flag (0 to 1).
    wind: f32,
    /// The jelly's last hit: x, y, z and start (seconds).
    jelly: [f32; 4],
    /// A shatter burst's start in seconds, or below 0.
    shatter: f32,
    /// The heat haze (0 to 1) and the lens's refraction (0 to 1).
    heat: f32,
    refract: f32,
    /// The force field's last pulse (seconds).
    pulse: f32,
    /// Whether the boss is outlined and emissive pieces bloom.
    pub outline: bool,
    pub bloom: bool,
    /// The storm's last burst (seconds), and the fire's ignition, if it burns.
    pub storm: f32,
    pub fire: Option<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            declared: 0,
            rich: 0,
            computes: 0,
            thermal: false,
            cut: None,
            boom: [0.0; 3],
            heartbeat: None,
            hit: [0.0; 4],
            corruption: None,
            centres: [[0.0; 3]; 9],
            quarter: 0.0,
            players: [[0.0; 2]; 4],
            player_count: 0.0,
            portal_near: 0.0,
            portal_spin: Phase {
                rate: portal_rate(0.0),
                offset: 0.0,
            },
            wind: 0.5,
            jelly: [0.0, 0.0, 0.0, -100.0],
            shatter: -1.0,
            heat: 0.6,
            refract: 0.7,
            pulse: -100.0,
            outline: true,
            bloom: false,
            storm: -100.0,
            fire: None,
        }
    }
}

impl State {
    /// Applies parameter operation `op` with `args` and returns the materials it changed.
    pub fn apply(&mut self, op: u16, args: [i64; 4]) -> &'static [u32] {
        let blocks = |value: i64| value as f32 / MILLI;
        match op {
            operations::CUT => {
                self.cut = (args[0] >= 0).then_some(args[0].min(1000));
                &[WRAITH]
            }
            operations::BOOM => {
                self.boom = [blocks(args[0]), blocks(args[1]), blocks(args[2])];
                &[ARENA]
            }
            operations::HEARTBEAT => {
                let at = args[1] as f64 / f64::from(MILLI);
                let rate = beat_rate(args[0].min(1000) as f32 / 1000.0);
                self.heartbeat = match (args[0] >= 0, self.heartbeat) {
                    (false, _) => None,
                    (true, Some(mut beat)) => {
                        beat.retime(at, rate);
                        Some(beat)
                    }
                    (true, None) => Some(Phase::starting(at, rate)),
                };
                &[ARENA]
            }
            operations::HIT => {
                self.hit = [
                    blocks(args[0]),
                    blocks(args[1]),
                    blocks(args[2]),
                    blocks(args[3]),
                ];
                &[METAL]
            }
            operations::CORRUPT => {
                self.corruption = (args[3] >= 0).then(|| {
                    [
                        blocks(args[0]),
                        blocks(args[1]),
                        blocks(args[2]),
                        blocks(args[3]),
                    ]
                });
                &[CORRUPTION]
            }
            operations::LAYOUT => {
                let Some(centre) = self.centres.get_mut(args[0].clamp(0, 8) as usize) else {
                    return &[];
                };
                *centre = [blocks(args[1]), blocks(args[2]), blocks(args[3])];
                &[
                    ARENA,
                    BLACK_HOLE,
                    PAINTING,
                    SCREEN,
                    PORTAL,
                    FLAG,
                    FIRE,
                    FORCE_FIELD,
                    WAVE_GRASS,
                ]
            }
            operations::FACING => {
                self.quarter = args[0].rem_euclid(4) as f32;
                &[PAINTING, SCREEN, PORTAL, FLAG, FIRE, WAVE_GRASS]
            }
            operations::RADAR => {
                if let Some(player) = self.players.get_mut(args[0].clamp(0, 3) as usize) {
                    *player = [blocks(args[1]), blocks(args[2])];
                }
                self.player_count = args[3].clamp(0, 4) as f32;
                &[SCREEN]
            }
            operations::PORTAL_DISTANCE => {
                self.portal_near = portal_near(blocks(args[0]));
                let at = args[1] as f64 / f64::from(MILLI);
                self.portal_spin.retime(at, portal_rate(self.portal_near));
                &[PORTAL]
            }
            operations::WIND => {
                self.wind = blocks(args[0]).clamp(0.0, 1.0);
                &[WAVE_GRASS, FLAG]
            }
            operations::JELLY_HIT => {
                self.jelly = [
                    blocks(args[0]),
                    blocks(args[1]),
                    blocks(args[2]),
                    blocks(args[3]),
                ];
                &[JELLY]
            }
            operations::SHATTER => {
                self.shatter = blocks(args[0]);
                &[SHATTER]
            }
            operations::HEAT => {
                self.heat = blocks(args[0]).clamp(0.0, 1.0);
                &[HAZE]
            }
            operations::REFRACT => {
                self.refract = blocks(args[0]).clamp(0.0, 1.0);
                &[LENS]
            }
            operations::PULSE => {
                self.pulse = blocks(args[0]);
                &[FORCE_FIELD]
            }
            operations::OUTLINE => {
                self.outline = args[0] == 1;
                &[BOSS]
            }
            operations::BLOOM => {
                self.bloom = args[0] == 1;
                &[
                    DIAMOND, REDSTONE, LAVA_LAMP, SCREEN, RUNE, PORTAL, BOSS, STORM, FIRE,
                ]
            }
            operations::STORM => {
                self.storm = blocks(args[0]);
                &[STORM]
            }
            operations::FIRE => {
                self.fire = (args[0] >= 0).then(|| blocks(args[0]));
                &[FIRE]
            }
            _ => &[],
        }
    }

    /// Whether material `id` was declared in its rich form.
    pub fn is_rich(&self, id: u32) -> bool {
        self.rich & (1 << id) != 0
    }

    /// Material `id`'s parameters.
    /// Material `id`'s parameters, for its `rich` form or its plain one.
    fn params(&self, id: u32, rich: bool) -> Vec<f32> {
        let angle = self.quarter * std::f32::consts::FRAC_PI_2;
        // The layout faces the player, whose forward is the quarter's direction.
        let front = [angle.sin(), 0.0, -angle.cos()];
        match id {
            WRAITH => {
                let cut = self.cut.map_or(-1.0, |cut| cut as f32 / 1000.0);
                vec![0.08, 1.0, 0.42, 0.08, 6.0, cut]
            }
            HOLOGRAM => vec![0.25, 0.85, 1.0, 8.0],
            VOID_WALKER => vec![0.75, 0.55, 1.0, 1.0],
            LENTICULAR => vec![0.45],
            METAL => self.hit.to_vec(),
            THERMAL => vec![0.85, 1.0],
            BOTTLE => vec![24.0, 0.12],
            AQUARIUM => vec![0.0, 3.0],
            MINESHAFT => vec![1.0, 10.0],
            ARENA => {
                let [x, z, start] = self.boom;
                let centre = self.centres[0];
                let (mode, beat) = self
                    .heartbeat
                    .map_or((0.0, Phase::default()), |beat| (1.0, beat));
                vec![
                    x,
                    z,
                    start,
                    mode,
                    beat.rate,
                    centre[0],
                    centre[2],
                    beat.offset.rem_euclid(1.0) as f32,
                ]
            }
            BLACK_HOLE => {
                let [x, y, z] = self.centres[1];
                vec![x, y, z, 0.16, 1.5, PIXELS_PER_RADIAN]
            }
            PAINTING => {
                let [x, y, z] = self.centres[2];
                vec![x, y, z, front[0], front[1], front[2]]
            }
            DIAMOND => vec![0.0, 1.0],
            REDSTONE => vec![1.0, 1.0],
            CORRUPTION => match self.corruption {
                Some([x, y, z, start]) => vec![x, y, z, start, 1.2],
                None => vec![0.0, 0.0, 0.0, -1.0, 1.2],
            },
            SNOW_GLOBE => vec![3.0, 0.1],
            LAVA_LAMP => vec![1.0, 0.55, 0.15, 1.0],
            SCREEN => {
                let [x, y, z] = self.centres[3];
                let mut params = vec![x, y, z, self.quarter, 0.5];
                params.extend(self.players.iter().flatten());
                params.push(self.player_count);
                params
            }
            RUNE => vec![0.15, 0.3, 0.8, 1.0],
            PORTAL => {
                let [x, y, z] = self.centres[4];
                vec![
                    x,
                    y,
                    z,
                    self.quarter,
                    self.portal_near,
                    0.45,
                    0.15,
                    0.85,
                    self.portal_spin.rate,
                    self.portal_spin.offset as f32,
                ]
            }
            WAVE_GRASS => {
                let [x, y, z] = self.centres[8];
                vec![self.wind, 1.2, x, y, z, self.quarter]
            }
            FLAG => {
                let [x, y, z] = self.centres[5];
                vec![x, y, z, self.quarter, self.wind]
            }
            JELLY => self.jelly.to_vec(),
            SHATTER => vec![4.0, self.shatter],
            LENS => vec![self.refract, PIXELS_PER_RADIAN, f32::from(u8::from(rich))],
            HAZE => vec![self.heat * f32::from(u8::from(rich))],
            FORCE_FIELD => {
                let [x, y, z] = self.centres[7];
                vec![x, y, z, self.pulse]
            }
            BOSS => {
                let beat = self.heartbeat.unwrap_or(Phase {
                    rate: 0.8,
                    offset: 0.0,
                });
                vec![beat.rate, beat.offset.rem_euclid(1.0) as f32]
            }
            STORM => vec![self.storm],
            FIRE => {
                let [x, y, z] = self.centres[6];
                vec![
                    x,
                    y,
                    z,
                    self.quarter,
                    (FIRE_WIDTH / 16) as f32,
                    (FIRE_HEIGHT / 16) as f32,
                ]
            }
            _ => Vec::new(),
        }
    }
}

/// Screen pixels a bend of one radian moves the scene by, for the lens and the black hole.
const PIXELS_PER_RADIAN: f32 = 700.0;

/// Material `id`'s plain declaration under `state`, which needs no effects permission.
pub fn spec(id: u32, state: &State) -> MaterialSpec {
    let effect = &EFFECTS[id as usize];
    MaterialSpec {
        source: effect.source.into(),
        textures: effect.textures.iter().map(|path| (*path).into()).collect(),
        params: state.params(id, false),
        depth: Depth::Test,
        blend: effect.blend,
    }
}

/// Material `id`'s rich declaration and effects under `state`, if it has a rich form.
pub fn rich_spec(id: u32, state: &State) -> Option<(MaterialSpec, wme::Effects)> {
    let rich = EFFECTS[id as usize].rich.as_ref()?;
    let mut spec = spec(id, state);
    spec.params = state.params(id, true);
    if let Some(source) = rich.source {
        spec.source = source.into();
    }
    Some((spec, (rich.effects)(state)))
}
