// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use crate::wire;

// Domain: actor

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorEventType {
    #[default]
    None,
    Jump,
    Hurt,
    Death,
    StartAttacking,
    StopAttacking,
    TamingFailed,
    TamingSucceeded,
    ShakeWetness,
    EatGrass,
    FishHookBubble,
    FishHookFishPos,
    FishHookHookTime,
    FishHookTease,
    SquidFleeing,
    ZombieConverting,
    PlayAmbient,
    SpawnAlive,
    StartOfferFlower,
    StopOfferFlower,
    LoveHearts,
    VillagerAngry,
    VillagerHappy,
    WitchHatMagic,
    FireworksExplode,
    InLoveHearts,
    SilverfishMergeAnim,
    GuardianAttackSound,
    DrinkPotion,
    ThrowPotion,
    PrimeTntCart,
    PrimeCreeper,
    AirSupply,
    DeprecatedAddPlayerLevels,
    GuardianMiningFatigue,
    AgentSwingArm,
    DragonStartDeathAnim,
    GroundDust,
    Shake,
    Feed,
    BabyAge,
    InstantDeath,
    NotifyTrade,
    LeashDestroyed,
    CaravanUpdated,
    TalismanActivate,
    DeprecatedUpdateStructureFeature,
    PlayerSpawnedMob,
    Puke,
    UpdateStackSize,
    StartSwimming,
    BalloonPop,
    TreasureHunt,
    SummonAgent,
    FinishedChargingItem,
    ActorGrowUp,
    VibrationDetected,
    DrinkMilk,
    ShakeWetnessStop,
    KineticDamageDealt,
    HurtWithoutReceivingDamage,
    Unknown(u8),
}

impl From<u8> for ActorEventType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Jump,
            2 => Self::Hurt,
            3 => Self::Death,
            4 => Self::StartAttacking,
            5 => Self::StopAttacking,
            6 => Self::TamingFailed,
            7 => Self::TamingSucceeded,
            8 => Self::ShakeWetness,
            10 => Self::EatGrass,
            11 => Self::FishHookBubble,
            12 => Self::FishHookFishPos,
            13 => Self::FishHookHookTime,
            14 => Self::FishHookTease,
            15 => Self::SquidFleeing,
            16 => Self::ZombieConverting,
            17 => Self::PlayAmbient,
            18 => Self::SpawnAlive,
            19 => Self::StartOfferFlower,
            20 => Self::StopOfferFlower,
            21 => Self::LoveHearts,
            22 => Self::VillagerAngry,
            23 => Self::VillagerHappy,
            24 => Self::WitchHatMagic,
            25 => Self::FireworksExplode,
            26 => Self::InLoveHearts,
            27 => Self::SilverfishMergeAnim,
            28 => Self::GuardianAttackSound,
            29 => Self::DrinkPotion,
            30 => Self::ThrowPotion,
            31 => Self::PrimeTntCart,
            32 => Self::PrimeCreeper,
            33 => Self::AirSupply,
            34 => Self::DeprecatedAddPlayerLevels,
            35 => Self::GuardianMiningFatigue,
            36 => Self::AgentSwingArm,
            37 => Self::DragonStartDeathAnim,
            38 => Self::GroundDust,
            39 => Self::Shake,
            57 => Self::Feed,
            60 => Self::BabyAge,
            61 => Self::InstantDeath,
            62 => Self::NotifyTrade,
            63 => Self::LeashDestroyed,
            64 => Self::CaravanUpdated,
            65 => Self::TalismanActivate,
            66 => Self::DeprecatedUpdateStructureFeature,
            67 => Self::PlayerSpawnedMob,
            68 => Self::Puke,
            69 => Self::UpdateStackSize,
            70 => Self::StartSwimming,
            71 => Self::BalloonPop,
            72 => Self::TreasureHunt,
            73 => Self::SummonAgent,
            74 => Self::FinishedChargingItem,
            76 => Self::ActorGrowUp,
            77 => Self::VibrationDetected,
            78 => Self::DrinkMilk,
            79 => Self::ShakeWetnessStop,
            80 => Self::KineticDamageDealt,
            81 => Self::HurtWithoutReceivingDamage,
            value => Self::Unknown(value),
        }
    }
}

impl ActorEventType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Jump => 1,
            Self::Hurt => 2,
            Self::Death => 3,
            Self::StartAttacking => 4,
            Self::StopAttacking => 5,
            Self::TamingFailed => 6,
            Self::TamingSucceeded => 7,
            Self::ShakeWetness => 8,
            Self::EatGrass => 10,
            Self::FishHookBubble => 11,
            Self::FishHookFishPos => 12,
            Self::FishHookHookTime => 13,
            Self::FishHookTease => 14,
            Self::SquidFleeing => 15,
            Self::ZombieConverting => 16,
            Self::PlayAmbient => 17,
            Self::SpawnAlive => 18,
            Self::StartOfferFlower => 19,
            Self::StopOfferFlower => 20,
            Self::LoveHearts => 21,
            Self::VillagerAngry => 22,
            Self::VillagerHappy => 23,
            Self::WitchHatMagic => 24,
            Self::FireworksExplode => 25,
            Self::InLoveHearts => 26,
            Self::SilverfishMergeAnim => 27,
            Self::GuardianAttackSound => 28,
            Self::DrinkPotion => 29,
            Self::ThrowPotion => 30,
            Self::PrimeTntCart => 31,
            Self::PrimeCreeper => 32,
            Self::AirSupply => 33,
            Self::DeprecatedAddPlayerLevels => 34,
            Self::GuardianMiningFatigue => 35,
            Self::AgentSwingArm => 36,
            Self::DragonStartDeathAnim => 37,
            Self::GroundDust => 38,
            Self::Shake => 39,
            Self::Feed => 57,
            Self::BabyAge => 60,
            Self::InstantDeath => 61,
            Self::NotifyTrade => 62,
            Self::LeashDestroyed => 63,
            Self::CaravanUpdated => 64,
            Self::TalismanActivate => 65,
            Self::DeprecatedUpdateStructureFeature => 66,
            Self::PlayerSpawnedMob => 67,
            Self::Puke => 68,
            Self::UpdateStackSize => 69,
            Self::StartSwimming => 70,
            Self::BalloonPop => 71,
            Self::TreasureHunt => 72,
            Self::SummonAgent => 73,
            Self::FinishedChargingItem => 74,
            Self::ActorGrowUp => 76,
            Self::VibrationDetected => 77,
            Self::DrinkMilk => 78,
            Self::ShakeWetnessStop => 79,
            Self::KineticDamageDealt => 80,
            Self::HurtWithoutReceivingDamage => 81,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorEventType> for u8 {
    fn from(value: ActorEventType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorEventType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorEventType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorLinkType {
    #[default]
    None,
    Riding,
    Passenger,
    Unknown(u8),
}

impl From<u8> for ActorLinkType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Riding,
            2 => Self::Passenger,
            value => Self::Unknown(value),
        }
    }
}

impl ActorLinkType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Riding => 1,
            Self::Passenger => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorLinkType> for u8 {
    fn from(value: ActorLinkType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorLinkType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorLinkType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ActorType {
    #[default]
    Undefined,
    ItemEntity,
    PrimedTnt,
    FallingBlock,
    MovingBlock,
    Experience,
    EyeOfEnder,
    EnderCrystal,
    FireworksRocket,
    FishingHook,
    Chalkboard,
    Painting,
    LeashKnot,
    BoatRideable,
    LightningBolt,
    AreaEffectCloud,
    Balloon,
    Shield,
    Lectern,
    OminousItemSpawner,
    Cushion,
    ChestBoatRideable,
    Mob,
    Npc,
    Agent,
    ArmorStand,
    TripodCamera,
    Player,
    Bee,
    Piglin,
    PiglinBrute,
    Allay,
    PathfinderMob,
    IronGolem,
    SnowGolem,
    WanderingTrader,
    CopperGolem,
    SulfurCube,
    Monster,
    Creeper,
    Slime,
    EnderMan,
    Ghast,
    LavaSlime,
    Blaze,
    Witch,
    Guardian,
    ElderGuardian,
    Dragon,
    Shulker,
    Vindicator,
    IllagerBeast,
    EvocationIllager,
    Vex,
    Pillager,
    ElderGuardianGhost,
    Warden,
    Breeze,
    Creaking,
    Animal,
    Chicken,
    Cow,
    Pig,
    Sheep,
    MushroomCow,
    Rabbit,
    PolarBear,
    Llama,
    Turtle,
    Panda,
    Fox,
    Hoglin,
    Strider,
    Goat,
    Axolotl,
    Frog,
    Camel,
    Sniffer,
    Armadillo,
    HappyGhast,
    TraderLlama,
    WaterAnimal,
    Squid,
    Dolphin,
    Pufferfish,
    Salmon,
    Tropicalfish,
    Fish,
    GlowSquid,
    Tadpole,
    Nautilus,
    TamableAnimal,
    Wolf,
    Ocelot,
    Parrot,
    Cat,
    Ambient,
    Bat,
    UndeadMonster,
    PigZombie,
    WitherBoss,
    Phantom,
    Zoglin,
    CamelHusk,
    ZombieNautilus,
    ZombieMonster,
    Zombie,
    ZombieVillager,
    Husk,
    Drowned,
    ZombieVillagerV2,
    Frostbite,
    Arthropod,
    Spider,
    Silverfish,
    CaveSpider,
    Endermite,
    Minecart,
    MinecartRideable,
    MinecartHopper,
    MinecartTnt,
    MinecartChest,
    MinecartFurnace,
    MinecartCommandBlock,
    SkeletonMonster,
    Skeleton,
    Stray,
    WitherSkeleton,
    Bogged,
    Parched,
    EquineAnimal,
    Horse,
    Donkey,
    Mule,
    SkeletonHorse,
    ZombieHorse,
    Projectile,
    ExperiencePotion,
    ShulkerBullet,
    DragonFireball,
    Snowball,
    ThrownEgg,
    LargeFireball,
    ThrownPotion,
    Enderpearl,
    WitherSkull,
    WitherSkullDangerous,
    SmallFireball,
    LingeringPotion,
    LlamaSpit,
    EvocationFang,
    IceBomb,
    BreezeWindChargeProjectile,
    WindChargeProjectile,
    IceBall,
    AbstractArrow,
    Trident,
    Arrow,
    VillagerBase,
    Villager,
    VillagerV2,
    Unknown(i32),
}

impl From<i32> for ActorType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Undefined,
            64 => Self::ItemEntity,
            65 => Self::PrimedTnt,
            66 => Self::FallingBlock,
            67 => Self::MovingBlock,
            69 => Self::Experience,
            70 => Self::EyeOfEnder,
            71 => Self::EnderCrystal,
            72 => Self::FireworksRocket,
            77 => Self::FishingHook,
            78 => Self::Chalkboard,
            83 => Self::Painting,
            88 => Self::LeashKnot,
            90 => Self::BoatRideable,
            93 => Self::LightningBolt,
            95 => Self::AreaEffectCloud,
            107 => Self::Balloon,
            117 => Self::Shield,
            119 => Self::Lectern,
            145 => Self::OminousItemSpawner,
            154 => Self::Cushion,
            218 => Self::ChestBoatRideable,
            256 => Self::Mob,
            307 => Self::Npc,
            312 => Self::Agent,
            317 => Self::ArmorStand,
            318 => Self::TripodCamera,
            319 => Self::Player,
            378 => Self::Bee,
            379 => Self::Piglin,
            383 => Self::PiglinBrute,
            390 => Self::Allay,
            768 => Self::PathfinderMob,
            788 => Self::IronGolem,
            789 => Self::SnowGolem,
            886 => Self::WanderingTrader,
            916 => Self::CopperGolem,
            921 => Self::SulfurCube,
            2816 => Self::Monster,
            2849 => Self::Creeper,
            2853 => Self::Slime,
            2854 => Self::EnderMan,
            2857 => Self::Ghast,
            2858 => Self::LavaSlime,
            2859 => Self::Blaze,
            2861 => Self::Witch,
            2865 => Self::Guardian,
            2866 => Self::ElderGuardian,
            2869 => Self::Dragon,
            2870 => Self::Shulker,
            2873 => Self::Vindicator,
            2875 => Self::IllagerBeast,
            2920 => Self::EvocationIllager,
            2921 => Self::Vex,
            2930 => Self::Pillager,
            2936 => Self::ElderGuardianGhost,
            2947 => Self::Warden,
            2956 => Self::Breeze,
            2962 => Self::Creaking,
            4864 => Self::Animal,
            4874 => Self::Chicken,
            4875 => Self::Cow,
            4876 => Self::Pig,
            4877 => Self::Sheep,
            4880 => Self::MushroomCow,
            4882 => Self::Rabbit,
            4892 => Self::PolarBear,
            4893 => Self::Llama,
            4938 => Self::Turtle,
            4977 => Self::Panda,
            4985 => Self::Fox,
            4988 => Self::Hoglin,
            4989 => Self::Strider,
            4992 => Self::Goat,
            4994 => Self::Axolotl,
            4996 => Self::Frog,
            5002 => Self::Camel,
            5003 => Self::Sniffer,
            5006 => Self::Armadillo,
            5011 => Self::HappyGhast,
            5021 => Self::TraderLlama,
            8960 => Self::WaterAnimal,
            8977 => Self::Squid,
            8991 => Self::Dolphin,
            9068 => Self::Pufferfish,
            9069 => Self::Salmon,
            9071 => Self::Tropicalfish,
            9072 => Self::Fish,
            9089 => Self::GlowSquid,
            9093 => Self::Tadpole,
            9109 => Self::Nautilus,
            21248 => Self::TamableAnimal,
            21262 => Self::Wolf,
            21270 => Self::Ocelot,
            21278 => Self::Parrot,
            21323 => Self::Cat,
            33024 => Self::Ambient,
            33043 => Self::Bat,
            68352 => Self::UndeadMonster,
            68388 => Self::PigZombie,
            68404 => Self::WitherBoss,
            68410 => Self::Phantom,
            68478 => Self::Zoglin,
            70552 => Self::CamelHusk,
            74646 => Self::ZombieNautilus,
            199424 => Self::ZombieMonster,
            199456 => Self::Zombie,
            199468 => Self::ZombieVillager,
            199471 => Self::Husk,
            199534 => Self::Drowned,
            199540 => Self::ZombieVillagerV2,
            199579 => Self::Frostbite,
            264960 => Self::Arthropod,
            264995 => Self::Spider,
            264999 => Self::Silverfish,
            265000 => Self::CaveSpider,
            265015 => Self::Endermite,
            524288 => Self::Minecart,
            524372 => Self::MinecartRideable,
            524384 => Self::MinecartHopper,
            524385 => Self::MinecartTnt,
            524386 => Self::MinecartChest,
            524387 => Self::MinecartFurnace,
            524388 => Self::MinecartCommandBlock,
            1116928 => Self::SkeletonMonster,
            1116962 => Self::Skeleton,
            1116974 => Self::Stray,
            1116976 => Self::WitherSkeleton,
            1117072 => Self::Bogged,
            1117079 => Self::Parched,
            2118400 => Self::EquineAnimal,
            2118423 => Self::Horse,
            2118424 => Self::Donkey,
            2118425 => Self::Mule,
            2183962 => Self::SkeletonHorse,
            2183963 => Self::ZombieHorse,
            4194304 => Self::Projectile,
            4194372 => Self::ExperiencePotion,
            4194380 => Self::ShulkerBullet,
            4194383 => Self::DragonFireball,
            4194385 => Self::Snowball,
            4194386 => Self::ThrownEgg,
            4194389 => Self::LargeFireball,
            4194390 => Self::ThrownPotion,
            4194391 => Self::Enderpearl,
            4194393 => Self::WitherSkull,
            4194395 => Self::WitherSkullDangerous,
            4194398 => Self::SmallFireball,
            4194405 => Self::LingeringPotion,
            4194406 => Self::LlamaSpit,
            4194407 => Self::EvocationFang,
            4194410 => Self::IceBomb,
            4194445 => Self::BreezeWindChargeProjectile,
            4194447 => Self::WindChargeProjectile,
            4194460 => Self::IceBall,
            8388608 => Self::AbstractArrow,
            12582985 => Self::Trident,
            12582992 => Self::Arrow,
            16777984 => Self::VillagerBase,
            16777999 => Self::Villager,
            16778099 => Self::VillagerV2,
            value => Self::Unknown(value),
        }
    }
}

impl ActorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Undefined => 1,
            Self::ItemEntity => 64,
            Self::PrimedTnt => 65,
            Self::FallingBlock => 66,
            Self::MovingBlock => 67,
            Self::Experience => 69,
            Self::EyeOfEnder => 70,
            Self::EnderCrystal => 71,
            Self::FireworksRocket => 72,
            Self::FishingHook => 77,
            Self::Chalkboard => 78,
            Self::Painting => 83,
            Self::LeashKnot => 88,
            Self::BoatRideable => 90,
            Self::LightningBolt => 93,
            Self::AreaEffectCloud => 95,
            Self::Balloon => 107,
            Self::Shield => 117,
            Self::Lectern => 119,
            Self::OminousItemSpawner => 145,
            Self::Cushion => 154,
            Self::ChestBoatRideable => 218,
            Self::Mob => 256,
            Self::Npc => 307,
            Self::Agent => 312,
            Self::ArmorStand => 317,
            Self::TripodCamera => 318,
            Self::Player => 319,
            Self::Bee => 378,
            Self::Piglin => 379,
            Self::PiglinBrute => 383,
            Self::Allay => 390,
            Self::PathfinderMob => 768,
            Self::IronGolem => 788,
            Self::SnowGolem => 789,
            Self::WanderingTrader => 886,
            Self::CopperGolem => 916,
            Self::SulfurCube => 921,
            Self::Monster => 2816,
            Self::Creeper => 2849,
            Self::Slime => 2853,
            Self::EnderMan => 2854,
            Self::Ghast => 2857,
            Self::LavaSlime => 2858,
            Self::Blaze => 2859,
            Self::Witch => 2861,
            Self::Guardian => 2865,
            Self::ElderGuardian => 2866,
            Self::Dragon => 2869,
            Self::Shulker => 2870,
            Self::Vindicator => 2873,
            Self::IllagerBeast => 2875,
            Self::EvocationIllager => 2920,
            Self::Vex => 2921,
            Self::Pillager => 2930,
            Self::ElderGuardianGhost => 2936,
            Self::Warden => 2947,
            Self::Breeze => 2956,
            Self::Creaking => 2962,
            Self::Animal => 4864,
            Self::Chicken => 4874,
            Self::Cow => 4875,
            Self::Pig => 4876,
            Self::Sheep => 4877,
            Self::MushroomCow => 4880,
            Self::Rabbit => 4882,
            Self::PolarBear => 4892,
            Self::Llama => 4893,
            Self::Turtle => 4938,
            Self::Panda => 4977,
            Self::Fox => 4985,
            Self::Hoglin => 4988,
            Self::Strider => 4989,
            Self::Goat => 4992,
            Self::Axolotl => 4994,
            Self::Frog => 4996,
            Self::Camel => 5002,
            Self::Sniffer => 5003,
            Self::Armadillo => 5006,
            Self::HappyGhast => 5011,
            Self::TraderLlama => 5021,
            Self::WaterAnimal => 8960,
            Self::Squid => 8977,
            Self::Dolphin => 8991,
            Self::Pufferfish => 9068,
            Self::Salmon => 9069,
            Self::Tropicalfish => 9071,
            Self::Fish => 9072,
            Self::GlowSquid => 9089,
            Self::Tadpole => 9093,
            Self::Nautilus => 9109,
            Self::TamableAnimal => 21248,
            Self::Wolf => 21262,
            Self::Ocelot => 21270,
            Self::Parrot => 21278,
            Self::Cat => 21323,
            Self::Ambient => 33024,
            Self::Bat => 33043,
            Self::UndeadMonster => 68352,
            Self::PigZombie => 68388,
            Self::WitherBoss => 68404,
            Self::Phantom => 68410,
            Self::Zoglin => 68478,
            Self::CamelHusk => 70552,
            Self::ZombieNautilus => 74646,
            Self::ZombieMonster => 199424,
            Self::Zombie => 199456,
            Self::ZombieVillager => 199468,
            Self::Husk => 199471,
            Self::Drowned => 199534,
            Self::ZombieVillagerV2 => 199540,
            Self::Frostbite => 199579,
            Self::Arthropod => 264960,
            Self::Spider => 264995,
            Self::Silverfish => 264999,
            Self::CaveSpider => 265000,
            Self::Endermite => 265015,
            Self::Minecart => 524288,
            Self::MinecartRideable => 524372,
            Self::MinecartHopper => 524384,
            Self::MinecartTnt => 524385,
            Self::MinecartChest => 524386,
            Self::MinecartFurnace => 524387,
            Self::MinecartCommandBlock => 524388,
            Self::SkeletonMonster => 1116928,
            Self::Skeleton => 1116962,
            Self::Stray => 1116974,
            Self::WitherSkeleton => 1116976,
            Self::Bogged => 1117072,
            Self::Parched => 1117079,
            Self::EquineAnimal => 2118400,
            Self::Horse => 2118423,
            Self::Donkey => 2118424,
            Self::Mule => 2118425,
            Self::SkeletonHorse => 2183962,
            Self::ZombieHorse => 2183963,
            Self::Projectile => 4194304,
            Self::ExperiencePotion => 4194372,
            Self::ShulkerBullet => 4194380,
            Self::DragonFireball => 4194383,
            Self::Snowball => 4194385,
            Self::ThrownEgg => 4194386,
            Self::LargeFireball => 4194389,
            Self::ThrownPotion => 4194390,
            Self::Enderpearl => 4194391,
            Self::WitherSkull => 4194393,
            Self::WitherSkullDangerous => 4194395,
            Self::SmallFireball => 4194398,
            Self::LingeringPotion => 4194405,
            Self::LlamaSpit => 4194406,
            Self::EvocationFang => 4194407,
            Self::IceBomb => 4194410,
            Self::BreezeWindChargeProjectile => 4194445,
            Self::WindChargeProjectile => 4194447,
            Self::IceBall => 4194460,
            Self::AbstractArrow => 8388608,
            Self::Trident => 12582985,
            Self::Arrow => 12582992,
            Self::VillagerBase => 16777984,
            Self::Villager => 16777999,
            Self::VillagerV2 => 16778099,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ActorType> for i32 {
    fn from(value: ActorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ActorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PassengerOfBlockArgumentsEmoteType {
    #[default]
    Standing,
    Riding,
    Laying,
    Unknown(u8),
}

impl From<u8> for PassengerOfBlockArgumentsEmoteType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Standing,
            1 => Self::Riding,
            2 => Self::Laying,
            value => Self::Unknown(value),
        }
    }
}

impl PassengerOfBlockArgumentsEmoteType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Standing => 0,
            Self::Riding => 1,
            Self::Laying => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PassengerOfBlockArgumentsEmoteType> for u8 {
    fn from(value: PassengerOfBlockArgumentsEmoteType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PassengerOfBlockArgumentsEmoteType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PassengerOfBlockArgumentsEmoteType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: attribute_layer

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EASNoiseAlignmentType {
    #[default]
    MinLocalTransitionEnd,
    Unknown(u8),
}

impl From<u8> for EASNoiseAlignmentType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::MinLocalTransitionEnd,
            value => Self::Unknown(value),
        }
    }
}

impl EASNoiseAlignmentType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::MinLocalTransitionEnd => 0,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EASNoiseAlignmentType> for u8 {
    fn from(value: EASNoiseAlignmentType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EASNoiseAlignmentType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EASNoiseAlignmentType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EasingType {
    #[default]
    Linear,
    Spring,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InQuint,
    OutQuint,
    InOutQuint,
    InSine,
    OutSine,
    InOutSine,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    InBounce,
    OutBounce,
    InOutBounce,
    InBack,
    OutBack,
    InOutBack,
    InElastic,
    OutElastic,
    InOutElastic,
    Unknown(i32),
}

impl From<i32> for EasingType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Linear,
            1 => Self::Spring,
            2 => Self::InQuad,
            3 => Self::OutQuad,
            4 => Self::InOutQuad,
            5 => Self::InCubic,
            6 => Self::OutCubic,
            7 => Self::InOutCubic,
            8 => Self::InQuart,
            9 => Self::OutQuart,
            10 => Self::InOutQuart,
            11 => Self::InQuint,
            12 => Self::OutQuint,
            13 => Self::InOutQuint,
            14 => Self::InSine,
            15 => Self::OutSine,
            16 => Self::InOutSine,
            17 => Self::InExpo,
            18 => Self::OutExpo,
            19 => Self::InOutExpo,
            20 => Self::InCirc,
            21 => Self::OutCirc,
            22 => Self::InOutCirc,
            23 => Self::InBounce,
            24 => Self::OutBounce,
            25 => Self::InOutBounce,
            26 => Self::InBack,
            27 => Self::OutBack,
            28 => Self::InOutBack,
            29 => Self::InElastic,
            30 => Self::OutElastic,
            31 => Self::InOutElastic,
            value => Self::Unknown(value),
        }
    }
}

impl EasingType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Linear => 0,
            Self::Spring => 1,
            Self::InQuad => 2,
            Self::OutQuad => 3,
            Self::InOutQuad => 4,
            Self::InCubic => 5,
            Self::OutCubic => 6,
            Self::InOutCubic => 7,
            Self::InQuart => 8,
            Self::OutQuart => 9,
            Self::InOutQuart => 10,
            Self::InQuint => 11,
            Self::OutQuint => 12,
            Self::InOutQuint => 13,
            Self::InSine => 14,
            Self::OutSine => 15,
            Self::InOutSine => 16,
            Self::InExpo => 17,
            Self::OutExpo => 18,
            Self::InOutExpo => 19,
            Self::InCirc => 20,
            Self::OutCirc => 21,
            Self::InOutCirc => 22,
            Self::InBounce => 23,
            Self::OutBounce => 24,
            Self::InOutBounce => 25,
            Self::InBack => 26,
            Self::OutBack => 27,
            Self::InOutBack => 28,
            Self::InElastic => 29,
            Self::OutElastic => 30,
            Self::InOutElastic => 31,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EasingType> for i32 {
    fn from(value: EasingType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EasingType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EasingType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: camera

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistAction {
    #[default]
    Set,
    Clear,
    Unknown(u8),
}

impl From<u8> for CameraAimAssistAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Set,
            1 => Self::Clear,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Set => 0,
            Self::Clear => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistAction> for u8 {
    fn from(value: CameraAimAssistAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistPresetOperation {
    #[default]
    Set,
    AddToExisting,
    Unknown(u8),
}

impl From<u8> for CameraAimAssistPresetOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Set,
            1 => Self::AddToExisting,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistPresetOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Set => 0,
            Self::AddToExisting => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistPresetOperation> for u8 {
    fn from(value: CameraAimAssistPresetOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistPresetOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistPresetOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraAimAssistTargetMode {
    #[default]
    Angle,
    Distance,
    Unknown(i32),
}

impl From<i32> for CameraAimAssistTargetMode {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Angle,
            1 => Self::Distance,
            value => Self::Unknown(value),
        }
    }
}

impl CameraAimAssistTargetMode {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Angle => 0,
            Self::Distance => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraAimAssistTargetMode> for i32 {
    fn from(value: CameraAimAssistTargetMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraAimAssistTargetMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistTargetMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraPresetAudioListener {
    #[default]
    Camera,
    Player,
    Unknown(u8),
}

impl From<u8> for CameraPresetAudioListener {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Camera,
            1 => Self::Player,
            value => Self::Unknown(value),
        }
    }
}

impl CameraPresetAudioListener {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Camera => 0,
            Self::Player => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraPresetAudioListener> for u8 {
    fn from(value: CameraPresetAudioListener) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraPresetAudioListener {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraPresetAudioListener {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraShakeAction {
    #[default]
    Add,
    Stop,
    Unknown(u8),
}

impl From<u8> for CameraShakeAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Stop,
            value => Self::Unknown(value),
        }
    }
}

impl CameraShakeAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Stop => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraShakeAction> for u8 {
    fn from(value: CameraShakeAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraShakeAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraShakeAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CameraShakeType {
    #[default]
    Positional,
    Rotational,
    Unknown(u8),
}

impl From<u8> for CameraShakeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Positional,
            1 => Self::Rotational,
            value => Self::Unknown(value),
        }
    }
}

impl CameraShakeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Positional => 0,
            Self::Rotational => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CameraShakeType> for u8 {
    fn from(value: CameraShakeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CameraShakeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraShakeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: command

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CommandPermissionLevel {
    #[default]
    Any,
    GameDirectors,
    Admin,
    Host,
    Owner,
    Internal,
    Unknown(u8),
}

impl From<u8> for CommandPermissionLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Any,
            1 => Self::GameDirectors,
            2 => Self::Admin,
            3 => Self::Host,
            4 => Self::Owner,
            5 => Self::Internal,
            value => Self::Unknown(value),
        }
    }
}

impl CommandPermissionLevel {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Any => 0,
            Self::GameDirectors => 1,
            Self::Admin => 2,
            Self::Host => 3,
            Self::Owner => 4,
            Self::Internal => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CommandPermissionLevel> for u8 {
    fn from(value: CommandPermissionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CommandPermissionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandPermissionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: container

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ContainerEnumName {
    #[default]
    AnvilInputContainer,
    AnvilMaterialContainer,
    AnvilResultPreviewContainer,
    SmithingTableInputContainer,
    SmithingTableMaterialContainer,
    SmithingTableResultPreviewContainer,
    ArmorContainer,
    LevelEntityContainer,
    BeaconPaymentContainer,
    BrewingStandInputContainer,
    BrewingStandResultContainer,
    BrewingStandFuelContainer,
    CombinedHotbarAndInventoryContainer,
    CraftingInputContainer,
    CraftingOutputPreviewContainer,
    RecipeConstructionContainer,
    RecipeNatureContainer,
    RecipeItemsContainer,
    RecipeSearchContainer,
    RecipeSearchBarContainer,
    RecipeEquipmentContainer,
    RecipeBookContainer,
    EnchantingInputContainer,
    EnchantingMaterialContainer,
    FurnaceFuelContainer,
    FurnaceIngredientContainer,
    FurnaceResultContainer,
    HorseEquipContainer,
    HotbarContainer,
    InventoryContainer,
    ShulkerBoxContainer,
    TradeIngredient1Container,
    TradeIngredient2Container,
    TradeResultPreviewContainer,
    OffhandContainer,
    CompoundCreatorInput,
    CompoundCreatorOutputPreview,
    ElementConstructorOutputPreview,
    MaterialReducerInput,
    MaterialReducerOutput,
    LabTableInput,
    LoomInputContainer,
    LoomDyeContainer,
    LoomMaterialContainer,
    LoomResultPreviewContainer,
    BlastFurnaceIngredientContainer,
    SmokerIngredientContainer,
    Trade2Ingredient1Container,
    Trade2Ingredient2Container,
    Trade2ResultPreviewContainer,
    GrindstoneInputContainer,
    GrindstoneAdditionalContainer,
    GrindstoneResultPreviewContainer,
    StonecutterInputContainer,
    StonecutterResultPreviewContainer,
    CartographyInputContainer,
    CartographyAdditionalContainer,
    CartographyResultPreviewContainer,
    BarrelContainer,
    CursorContainer,
    CreatedOutputContainer,
    SmithingTableTemplateContainer,
    CrafterLevelEntityContainer,
    DynamicContainer,
    RecipeFoodContainer,
    RecipeBlocksContainer,
    RecipeFurnaceItemsContainer,
    ReservedContainerA,
    ReservedContainerB,
    ReservedContainerC,
    ReservedContainerD,
    Unknown(u8),
}

impl From<u8> for ContainerEnumName {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::AnvilInputContainer,
            1 => Self::AnvilMaterialContainer,
            2 => Self::AnvilResultPreviewContainer,
            3 => Self::SmithingTableInputContainer,
            4 => Self::SmithingTableMaterialContainer,
            5 => Self::SmithingTableResultPreviewContainer,
            6 => Self::ArmorContainer,
            7 => Self::LevelEntityContainer,
            8 => Self::BeaconPaymentContainer,
            9 => Self::BrewingStandInputContainer,
            10 => Self::BrewingStandResultContainer,
            11 => Self::BrewingStandFuelContainer,
            12 => Self::CombinedHotbarAndInventoryContainer,
            13 => Self::CraftingInputContainer,
            14 => Self::CraftingOutputPreviewContainer,
            15 => Self::RecipeConstructionContainer,
            16 => Self::RecipeNatureContainer,
            17 => Self::RecipeItemsContainer,
            18 => Self::RecipeSearchContainer,
            19 => Self::RecipeSearchBarContainer,
            20 => Self::RecipeEquipmentContainer,
            21 => Self::RecipeBookContainer,
            22 => Self::EnchantingInputContainer,
            23 => Self::EnchantingMaterialContainer,
            24 => Self::FurnaceFuelContainer,
            25 => Self::FurnaceIngredientContainer,
            26 => Self::FurnaceResultContainer,
            27 => Self::HorseEquipContainer,
            28 => Self::HotbarContainer,
            29 => Self::InventoryContainer,
            30 => Self::ShulkerBoxContainer,
            31 => Self::TradeIngredient1Container,
            32 => Self::TradeIngredient2Container,
            33 => Self::TradeResultPreviewContainer,
            34 => Self::OffhandContainer,
            35 => Self::CompoundCreatorInput,
            36 => Self::CompoundCreatorOutputPreview,
            37 => Self::ElementConstructorOutputPreview,
            38 => Self::MaterialReducerInput,
            39 => Self::MaterialReducerOutput,
            40 => Self::LabTableInput,
            41 => Self::LoomInputContainer,
            42 => Self::LoomDyeContainer,
            43 => Self::LoomMaterialContainer,
            44 => Self::LoomResultPreviewContainer,
            45 => Self::BlastFurnaceIngredientContainer,
            46 => Self::SmokerIngredientContainer,
            47 => Self::Trade2Ingredient1Container,
            48 => Self::Trade2Ingredient2Container,
            49 => Self::Trade2ResultPreviewContainer,
            50 => Self::GrindstoneInputContainer,
            51 => Self::GrindstoneAdditionalContainer,
            52 => Self::GrindstoneResultPreviewContainer,
            53 => Self::StonecutterInputContainer,
            54 => Self::StonecutterResultPreviewContainer,
            55 => Self::CartographyInputContainer,
            56 => Self::CartographyAdditionalContainer,
            57 => Self::CartographyResultPreviewContainer,
            58 => Self::BarrelContainer,
            59 => Self::CursorContainer,
            60 => Self::CreatedOutputContainer,
            61 => Self::SmithingTableTemplateContainer,
            62 => Self::CrafterLevelEntityContainer,
            63 => Self::DynamicContainer,
            64 => Self::RecipeFoodContainer,
            65 => Self::RecipeBlocksContainer,
            66 => Self::RecipeFurnaceItemsContainer,
            67 => Self::ReservedContainerA,
            68 => Self::ReservedContainerB,
            69 => Self::ReservedContainerC,
            70 => Self::ReservedContainerD,
            value => Self::Unknown(value),
        }
    }
}

impl ContainerEnumName {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::AnvilInputContainer => 0,
            Self::AnvilMaterialContainer => 1,
            Self::AnvilResultPreviewContainer => 2,
            Self::SmithingTableInputContainer => 3,
            Self::SmithingTableMaterialContainer => 4,
            Self::SmithingTableResultPreviewContainer => 5,
            Self::ArmorContainer => 6,
            Self::LevelEntityContainer => 7,
            Self::BeaconPaymentContainer => 8,
            Self::BrewingStandInputContainer => 9,
            Self::BrewingStandResultContainer => 10,
            Self::BrewingStandFuelContainer => 11,
            Self::CombinedHotbarAndInventoryContainer => 12,
            Self::CraftingInputContainer => 13,
            Self::CraftingOutputPreviewContainer => 14,
            Self::RecipeConstructionContainer => 15,
            Self::RecipeNatureContainer => 16,
            Self::RecipeItemsContainer => 17,
            Self::RecipeSearchContainer => 18,
            Self::RecipeSearchBarContainer => 19,
            Self::RecipeEquipmentContainer => 20,
            Self::RecipeBookContainer => 21,
            Self::EnchantingInputContainer => 22,
            Self::EnchantingMaterialContainer => 23,
            Self::FurnaceFuelContainer => 24,
            Self::FurnaceIngredientContainer => 25,
            Self::FurnaceResultContainer => 26,
            Self::HorseEquipContainer => 27,
            Self::HotbarContainer => 28,
            Self::InventoryContainer => 29,
            Self::ShulkerBoxContainer => 30,
            Self::TradeIngredient1Container => 31,
            Self::TradeIngredient2Container => 32,
            Self::TradeResultPreviewContainer => 33,
            Self::OffhandContainer => 34,
            Self::CompoundCreatorInput => 35,
            Self::CompoundCreatorOutputPreview => 36,
            Self::ElementConstructorOutputPreview => 37,
            Self::MaterialReducerInput => 38,
            Self::MaterialReducerOutput => 39,
            Self::LabTableInput => 40,
            Self::LoomInputContainer => 41,
            Self::LoomDyeContainer => 42,
            Self::LoomMaterialContainer => 43,
            Self::LoomResultPreviewContainer => 44,
            Self::BlastFurnaceIngredientContainer => 45,
            Self::SmokerIngredientContainer => 46,
            Self::Trade2Ingredient1Container => 47,
            Self::Trade2Ingredient2Container => 48,
            Self::Trade2ResultPreviewContainer => 49,
            Self::GrindstoneInputContainer => 50,
            Self::GrindstoneAdditionalContainer => 51,
            Self::GrindstoneResultPreviewContainer => 52,
            Self::StonecutterInputContainer => 53,
            Self::StonecutterResultPreviewContainer => 54,
            Self::CartographyInputContainer => 55,
            Self::CartographyAdditionalContainer => 56,
            Self::CartographyResultPreviewContainer => 57,
            Self::BarrelContainer => 58,
            Self::CursorContainer => 59,
            Self::CreatedOutputContainer => 60,
            Self::SmithingTableTemplateContainer => 61,
            Self::CrafterLevelEntityContainer => 62,
            Self::DynamicContainer => 63,
            Self::RecipeFoodContainer => 64,
            Self::RecipeBlocksContainer => 65,
            Self::RecipeFurnaceItemsContainer => 66,
            Self::ReservedContainerA => 67,
            Self::ReservedContainerB => 68,
            Self::ReservedContainerC => 69,
            Self::ReservedContainerD => 70,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ContainerEnumName> for u8 {
    fn from(value: ContainerEnumName) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ContainerEnumName {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerEnumName {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: creative

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CreativeItemCategory {
    #[default]
    Construction,
    Nature,
    Equipment,
    Items,
    ItemCommandOnly,
    Unknown(u8),
}

impl From<u8> for CreativeItemCategory {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Construction,
            2 => Self::Nature,
            3 => Self::Equipment,
            4 => Self::Items,
            5 => Self::ItemCommandOnly,
            value => Self::Unknown(value),
        }
    }
}

impl CreativeItemCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Construction => 1,
            Self::Nature => 2,
            Self::Equipment => 3,
            Self::Items => 4,
            Self::ItemCommandOnly => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CreativeItemCategory> for u8 {
    fn from(value: CreativeItemCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CreativeItemCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreativeItemCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: education

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EducationEditionOffer {
    #[default]
    None,
    RestOfWorld,
    ChinaDeprecated,
    Unknown(u32),
}

impl From<u32> for EducationEditionOffer {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::RestOfWorld,
            2 => Self::ChinaDeprecated,
            value => Self::Unknown(value),
        }
    }
}

impl EducationEditionOffer {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::RestOfWorld => 1,
            Self::ChinaDeprecated => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EducationEditionOffer> for u32 {
    fn from(value: EducationEditionOffer) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EducationEditionOffer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EducationEditionOffer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: enchant

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EnchantType {
    #[default]
    Protection,
    FireProtection,
    FeatherFalling,
    BlastProtection,
    ProjectileProtection,
    Thorns,
    Respiration,
    DepthStrider,
    AquaAffinity,
    Sharpness,
    Smite,
    BaneOfArthropods,
    Knockback,
    FireAspect,
    Looting,
    Efficiency,
    SilkTouch,
    Unbreaking,
    Fortune,
    Power,
    Punch,
    Flame,
    Infinity,
    LuckOfTheSea,
    Lure,
    FrostWalker,
    Mending,
    CurseOfBinding,
    CurseOfVanishing,
    Impaling,
    Riptide,
    Loyalty,
    Channeling,
    Multishot,
    Piercing,
    QuickCharge,
    SoulSpeed,
    SwiftSneak,
    WindBurst,
    Density,
    Breach,
    Lunge,
    NumEnchantments,
    InvalidEnchantment,
    Unknown(u8),
}

impl From<u8> for EnchantType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Protection,
            1 => Self::FireProtection,
            2 => Self::FeatherFalling,
            3 => Self::BlastProtection,
            4 => Self::ProjectileProtection,
            5 => Self::Thorns,
            6 => Self::Respiration,
            7 => Self::DepthStrider,
            8 => Self::AquaAffinity,
            9 => Self::Sharpness,
            10 => Self::Smite,
            11 => Self::BaneOfArthropods,
            12 => Self::Knockback,
            13 => Self::FireAspect,
            14 => Self::Looting,
            15 => Self::Efficiency,
            16 => Self::SilkTouch,
            17 => Self::Unbreaking,
            18 => Self::Fortune,
            19 => Self::Power,
            20 => Self::Punch,
            21 => Self::Flame,
            22 => Self::Infinity,
            23 => Self::LuckOfTheSea,
            24 => Self::Lure,
            25 => Self::FrostWalker,
            26 => Self::Mending,
            27 => Self::CurseOfBinding,
            28 => Self::CurseOfVanishing,
            29 => Self::Impaling,
            30 => Self::Riptide,
            31 => Self::Loyalty,
            32 => Self::Channeling,
            33 => Self::Multishot,
            34 => Self::Piercing,
            35 => Self::QuickCharge,
            36 => Self::SoulSpeed,
            37 => Self::SwiftSneak,
            38 => Self::WindBurst,
            39 => Self::Density,
            40 => Self::Breach,
            41 => Self::Lunge,
            42 => Self::NumEnchantments,
            43 => Self::InvalidEnchantment,
            value => Self::Unknown(value),
        }
    }
}

impl EnchantType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Protection => 0,
            Self::FireProtection => 1,
            Self::FeatherFalling => 2,
            Self::BlastProtection => 3,
            Self::ProjectileProtection => 4,
            Self::Thorns => 5,
            Self::Respiration => 6,
            Self::DepthStrider => 7,
            Self::AquaAffinity => 8,
            Self::Sharpness => 9,
            Self::Smite => 10,
            Self::BaneOfArthropods => 11,
            Self::Knockback => 12,
            Self::FireAspect => 13,
            Self::Looting => 14,
            Self::Efficiency => 15,
            Self::SilkTouch => 16,
            Self::Unbreaking => 17,
            Self::Fortune => 18,
            Self::Power => 19,
            Self::Punch => 20,
            Self::Flame => 21,
            Self::Infinity => 22,
            Self::LuckOfTheSea => 23,
            Self::Lure => 24,
            Self::FrostWalker => 25,
            Self::Mending => 26,
            Self::CurseOfBinding => 27,
            Self::CurseOfVanishing => 28,
            Self::Impaling => 29,
            Self::Riptide => 30,
            Self::Loyalty => 31,
            Self::Channeling => 32,
            Self::Multishot => 33,
            Self::Piercing => 34,
            Self::QuickCharge => 35,
            Self::SoulSpeed => 36,
            Self::SwiftSneak => 37,
            Self::WindBurst => 38,
            Self::Density => 39,
            Self::Breach => 40,
            Self::Lunge => 41,
            Self::NumEnchantments => 42,
            Self::InvalidEnchantment => 43,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EnchantType> for u8 {
    fn from(value: EnchantType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EnchantType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EnchantType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: furnace

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceLayout {
    #[default]
    None,
    InventoryOnly,
    Default,
    Unknown(i32),
}

impl From<i32> for FurnaceLayout {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::InventoryOnly,
            2 => Self::Default,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceLayout {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::InventoryOnly => 1,
            Self::Default => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceLayout> for i32 {
    fn from(value: FurnaceLayout) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceLayout {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceLayout {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceLeftTabIndex {
    #[default]
    None,
    RecipeFood,
    RecipeItems,
    RecipeBlocks,
    RecipeSearch,
    Inventory,
    Unknown(i32),
}

impl From<i32> for FurnaceLeftTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::RecipeFood,
            2 => Self::RecipeItems,
            3 => Self::RecipeBlocks,
            4 => Self::RecipeSearch,
            5 => Self::Inventory,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceLeftTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::RecipeFood => 1,
            Self::RecipeItems => 2,
            Self::RecipeBlocks => 3,
            Self::RecipeSearch => 4,
            Self::Inventory => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceLeftTabIndex> for i32 {
    fn from(value: FurnaceLeftTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceLeftTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceLeftTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FurnaceType {
    #[default]
    None,
    Furnace,
    Blast,
    Smoker,
    Unknown(u8),
}

impl From<u8> for FurnaceType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Furnace,
            2 => Self::Blast,
            3 => Self::Smoker,
            value => Self::Unknown(value),
        }
    }
}

impl FurnaceType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Furnace => 1,
            Self::Blast => 2,
            Self::Smoker => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<FurnaceType> for u8 {
    fn from(value: FurnaceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for FurnaceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: inventory

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HandSlot {
    #[default]
    Mainhand,
    Offhand,
    Unknown(u8),
}

impl From<u8> for HandSlot {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Mainhand,
            1 => Self::Offhand,
            value => Self::Unknown(value),
        }
    }
}

impl HandSlot {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Mainhand => 0,
            Self::Offhand => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HandSlot> for u8 {
    fn from(value: HandSlot) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HandSlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HandSlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryLayout {
    #[default]
    None,
    InventoryOnly,
    Default,
    RecipeBookOnly,
    Unknown(i32),
}

impl From<i32> for InventoryLayout {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::InventoryOnly,
            2 => Self::Default,
            3 => Self::RecipeBookOnly,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryLayout {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::InventoryOnly => 1,
            Self::Default => 2,
            Self::RecipeBookOnly => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryLayout> for i32 {
    fn from(value: InventoryLayout) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryLayout {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryLayout {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryLeftTabIndex {
    #[default]
    None,
    RecipeConstruction,
    RecipeEquipment,
    RecipeItems,
    RecipeNature,
    RecipeSearch,
    Survival,
    Unknown(i32),
}

impl From<i32> for InventoryLeftTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::RecipeConstruction,
            2 => Self::RecipeEquipment,
            3 => Self::RecipeItems,
            4 => Self::RecipeNature,
            5 => Self::RecipeSearch,
            6 => Self::Survival,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryLeftTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::RecipeConstruction => 1,
            Self::RecipeEquipment => 2,
            Self::RecipeItems => 3,
            Self::RecipeNature => 4,
            Self::RecipeSearch => 5,
            Self::Survival => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryLeftTabIndex> for i32 {
    fn from(value: InventoryLeftTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryLeftTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryLeftTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventoryRightTabIndex {
    #[default]
    None,
    FullScreen,
    Crafting,
    Armor,
    Unknown(i32),
}

impl From<i32> for InventoryRightTabIndex {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::FullScreen,
            2 => Self::Crafting,
            3 => Self::Armor,
            value => Self::Unknown(value),
        }
    }
}

impl InventoryRightTabIndex {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::FullScreen => 1,
            Self::Crafting => 2,
            Self::Armor => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventoryRightTabIndex> for i32 {
    fn from(value: InventoryRightTabIndex) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventoryRightTabIndex {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryRightTabIndex {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventorySourceInventorySourceFlags {
    #[default]
    NoFlag,
    WorldInteractionRandom,
    Unknown(u32),
}

impl From<u32> for InventorySourceInventorySourceFlags {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::NoFlag,
            1 => Self::WorldInteractionRandom,
            value => Self::Unknown(value),
        }
    }
}

impl InventorySourceInventorySourceFlags {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::NoFlag => 0,
            Self::WorldInteractionRandom => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventorySourceInventorySourceFlags> for u32 {
    fn from(value: InventorySourceInventorySourceFlags) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventorySourceInventorySourceFlags {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventorySourceInventorySourceFlags {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InventorySourceType {
    #[default]
    ContainerInventory,
    GlobalInventory,
    WorldInteraction,
    CreativeInventory,
    NonImplementedFeatureTodo,
    Unknown(u32),
}

impl From<u32> for InventorySourceType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::ContainerInventory,
            1 => Self::GlobalInventory,
            2 => Self::WorldInteraction,
            3 => Self::CreativeInventory,
            99999 => Self::NonImplementedFeatureTodo,
            value => Self::Unknown(value),
        }
    }
}

impl InventorySourceType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::ContainerInventory => 0,
            Self::GlobalInventory => 1,
            Self::WorldInteraction => 2,
            Self::CreativeInventory => 3,
            Self::NonImplementedFeatureTodo => 99999,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InventorySourceType> for u32 {
    fn from(value: InventorySourceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InventorySourceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventorySourceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: item

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemReleaseInventoryTransactionActionType {
    #[default]
    Release,
    Use,
    Unknown(i32),
}

impl From<i32> for ItemReleaseInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Release,
            1 => Self::Use,
            value => Self::Unknown(value),
        }
    }
}

impl ItemReleaseInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Release => 0,
            Self::Use => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemReleaseInventoryTransactionActionType> for i32 {
    fn from(value: ItemReleaseInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemReleaseInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemReleaseInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionActionType {
    #[default]
    Place,
    Use,
    Destroy,
    UseAsAttack,
    Unknown(i32),
}

impl From<i32> for ItemUseInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Place,
            1 => Self::Use,
            2 => Self::Destroy,
            3 => Self::UseAsAttack,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Place => 0,
            Self::Use => 1,
            Self::Destroy => 2,
            Self::UseAsAttack => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionActionType> for i32 {
    fn from(value: ItemUseInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionClientCooldownState {
    #[default]
    Off,
    On,
    Unknown(u8),
}

impl From<u8> for ItemUseInventoryTransactionClientCooldownState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::On,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionClientCooldownState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::On => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionClientCooldownState> for u8 {
    fn from(value: ItemUseInventoryTransactionClientCooldownState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionClientCooldownState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionClientCooldownState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionPredictedResult {
    #[default]
    Failure,
    Success,
    Unknown(u8),
}

impl From<u8> for ItemUseInventoryTransactionPredictedResult {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Failure,
            1 => Self::Success,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseInventoryTransactionPredictedResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Failure => 0,
            Self::Success => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionPredictedResult> for u8 {
    fn from(value: ItemUseInventoryTransactionPredictedResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionPredictedResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionPredictedResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseInventoryTransactionTriggerType {
    #[default]
    Unknown,
    PlayerInput,
    SimulationTick,
    Unknown2(u8),
}

impl From<u8> for ItemUseInventoryTransactionTriggerType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::PlayerInput,
            2 => Self::SimulationTick,
            value => Self::Unknown2(value),
        }
    }
}

impl ItemUseInventoryTransactionTriggerType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::PlayerInput => 1,
            Self::SimulationTick => 2,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<ItemUseInventoryTransactionTriggerType> for u8 {
    fn from(value: ItemUseInventoryTransactionTriggerType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseInventoryTransactionTriggerType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransactionTriggerType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemUseOnActorInventoryTransactionActionType {
    #[default]
    Interact,
    Attack,
    ItemInteract,
    Unknown(i32),
}

impl From<i32> for ItemUseOnActorInventoryTransactionActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Interact,
            1 => Self::Attack,
            2 => Self::ItemInteract,
            value => Self::Unknown(value),
        }
    }
}

impl ItemUseOnActorInventoryTransactionActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Interact => 0,
            Self::Attack => 1,
            Self::ItemInteract => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemUseOnActorInventoryTransactionActionType> for i32 {
    fn from(value: ItemUseOnActorInventoryTransactionActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemUseOnActorInventoryTransactionActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseOnActorInventoryTransactionActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemVersion {
    #[default]
    Legacy,
    DataDriven,
    None,
    Unknown(i32),
}

impl From<i32> for ItemVersion {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::DataDriven,
            2 => Self::None,
            value => Self::Unknown(value),
        }
    }
}

impl ItemVersion {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Legacy => 0,
            Self::DataDriven => 1,
            Self::None => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemVersion> for i32 {
    fn from(value: ItemVersion) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: item_descriptor

/// ItemDescriptor represents a type of item descriptor. This is one of the concrete types below. It is an
/// alias of Marshaler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemDescriptorType {
    #[default]
    Empty,
    ItemName,
    MoLang,
    ItemTag,
    Unknown(u8),
}

impl From<u8> for ItemDescriptorType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Empty,
            1 => Self::ItemName,
            2 => Self::MoLang,
            3 => Self::ItemTag,
            value => Self::Unknown(value),
        }
    }
}

impl ItemDescriptorType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::ItemName => 1,
            Self::MoLang => 2,
            Self::ItemTag => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemDescriptorType> for u8 {
    fn from(value: ItemDescriptorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemDescriptorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemDescriptorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: item_stack

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CursorItemDragState {
    #[default]
    Start,
    Stop,
    Unknown(u8),
}

impl From<u8> for CursorItemDragState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Start,
            1 => Self::Stop,
            value => Self::Unknown(value),
        }
    }
}

impl CursorItemDragState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Start => 0,
            Self::Stop => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CursorItemDragState> for u8 {
    fn from(value: CursorItemDragState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CursorItemDragState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CursorItemDragState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemStackNetResult {
    #[default]
    Success,
    Error,
    InvalidRequestActionType,
    ActionRequestNotAllowed,
    ScreenHandlerEndRequestFailed,
    ItemRequestActionHandlerCommitFailed,
    InvalidRequestCraftActionType,
    InvalidCraftRequest,
    InvalidCraftRequestScreen,
    InvalidCraftResult,
    InvalidCraftResultIndex,
    InvalidCraftResultItem,
    InvalidItemNetId,
    MissingCreatedOutputContainer,
    FailedToSetCreatedItemOutputSlot,
    RequestAlreadyInProgress,
    FailedToInitSparseContainer,
    ResultTransferFailed,
    ExpectedItemSlotNotFullyConsumed,
    ExpectedAnywhereItemNotFullyConsumed,
    ItemAlreadyConsumedFromSlot,
    ConsumedTooMuchFromSlot,
    MismatchSlotExpectedConsumedItem,
    MismatchSlotExpectedConsumedItemNetIdVariant,
    FailedToMatchExpectedSlotConsumedItem,
    FailedToMatchExpectedAllowedAnywhereConsumedItem,
    ConsumedItemOutOfAllowedSlotRange,
    ConsumedItemNotAllowed,
    PlayerNotInCreativeMode,
    InvalidExperimentalRecipeRequest,
    FailedToCraftCreative,
    FailedToGetLevelRecipe,
    FailedToFindRecipeByNetId,
    MismatchedCraftingSize,
    MissingInputSparseContainer,
    MismatchedRecipeForInputGridItems,
    EmptyCraftResults,
    FailedToEnchant,
    MissingInputItem,
    InsufficientPlayerLevelToEnchant,
    MissingMaterialItem,
    MissingActor,
    UnknownPrimaryEffect,
    PrimaryEffectOutOfRange,
    PrimaryEffectUnavailable,
    SecondaryEffectOutOfRange,
    SecondaryEffectUnavailable,
    DstContainerEqualToCreatedOutputContainer,
    DstContainerAndSlotEqualToSrcContainerAndSlot,
    FailedToValidateSrcSlot,
    FailedToValidateDstSlot,
    InvalidAdjustedAmount,
    InvalidItemSetType,
    InvalidTransferAmount,
    CannotSwapItem,
    CannotPlaceItem,
    UnhandledItemSetType,
    InvalidRemovedAmount,
    InvalidRegion,
    CannotDropItem,
    CannotDestroyItem,
    InvalidSourceContainer,
    ItemNotConsumed,
    InvalidNumCrafts,
    InvalidCraftResultStackSize,
    CannotRemoveItem,
    CannotConsumeItem,
    ScreenStackError,
    Unknown(u8),
}

impl From<u8> for ItemStackNetResult {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Success,
            1 => Self::Error,
            2 => Self::InvalidRequestActionType,
            3 => Self::ActionRequestNotAllowed,
            4 => Self::ScreenHandlerEndRequestFailed,
            5 => Self::ItemRequestActionHandlerCommitFailed,
            6 => Self::InvalidRequestCraftActionType,
            7 => Self::InvalidCraftRequest,
            8 => Self::InvalidCraftRequestScreen,
            9 => Self::InvalidCraftResult,
            10 => Self::InvalidCraftResultIndex,
            11 => Self::InvalidCraftResultItem,
            12 => Self::InvalidItemNetId,
            13 => Self::MissingCreatedOutputContainer,
            14 => Self::FailedToSetCreatedItemOutputSlot,
            15 => Self::RequestAlreadyInProgress,
            16 => Self::FailedToInitSparseContainer,
            17 => Self::ResultTransferFailed,
            18 => Self::ExpectedItemSlotNotFullyConsumed,
            19 => Self::ExpectedAnywhereItemNotFullyConsumed,
            20 => Self::ItemAlreadyConsumedFromSlot,
            21 => Self::ConsumedTooMuchFromSlot,
            22 => Self::MismatchSlotExpectedConsumedItem,
            23 => Self::MismatchSlotExpectedConsumedItemNetIdVariant,
            24 => Self::FailedToMatchExpectedSlotConsumedItem,
            25 => Self::FailedToMatchExpectedAllowedAnywhereConsumedItem,
            26 => Self::ConsumedItemOutOfAllowedSlotRange,
            27 => Self::ConsumedItemNotAllowed,
            28 => Self::PlayerNotInCreativeMode,
            29 => Self::InvalidExperimentalRecipeRequest,
            30 => Self::FailedToCraftCreative,
            31 => Self::FailedToGetLevelRecipe,
            32 => Self::FailedToFindRecipeByNetId,
            33 => Self::MismatchedCraftingSize,
            34 => Self::MissingInputSparseContainer,
            35 => Self::MismatchedRecipeForInputGridItems,
            36 => Self::EmptyCraftResults,
            37 => Self::FailedToEnchant,
            38 => Self::MissingInputItem,
            39 => Self::InsufficientPlayerLevelToEnchant,
            40 => Self::MissingMaterialItem,
            41 => Self::MissingActor,
            42 => Self::UnknownPrimaryEffect,
            43 => Self::PrimaryEffectOutOfRange,
            44 => Self::PrimaryEffectUnavailable,
            45 => Self::SecondaryEffectOutOfRange,
            46 => Self::SecondaryEffectUnavailable,
            47 => Self::DstContainerEqualToCreatedOutputContainer,
            48 => Self::DstContainerAndSlotEqualToSrcContainerAndSlot,
            49 => Self::FailedToValidateSrcSlot,
            50 => Self::FailedToValidateDstSlot,
            51 => Self::InvalidAdjustedAmount,
            52 => Self::InvalidItemSetType,
            53 => Self::InvalidTransferAmount,
            54 => Self::CannotSwapItem,
            55 => Self::CannotPlaceItem,
            56 => Self::UnhandledItemSetType,
            57 => Self::InvalidRemovedAmount,
            58 => Self::InvalidRegion,
            59 => Self::CannotDropItem,
            60 => Self::CannotDestroyItem,
            61 => Self::InvalidSourceContainer,
            62 => Self::ItemNotConsumed,
            63 => Self::InvalidNumCrafts,
            64 => Self::InvalidCraftResultStackSize,
            65 => Self::CannotRemoveItem,
            66 => Self::CannotConsumeItem,
            67 => Self::ScreenStackError,
            value => Self::Unknown(value),
        }
    }
}

impl ItemStackNetResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Error => 1,
            Self::InvalidRequestActionType => 2,
            Self::ActionRequestNotAllowed => 3,
            Self::ScreenHandlerEndRequestFailed => 4,
            Self::ItemRequestActionHandlerCommitFailed => 5,
            Self::InvalidRequestCraftActionType => 6,
            Self::InvalidCraftRequest => 7,
            Self::InvalidCraftRequestScreen => 8,
            Self::InvalidCraftResult => 9,
            Self::InvalidCraftResultIndex => 10,
            Self::InvalidCraftResultItem => 11,
            Self::InvalidItemNetId => 12,
            Self::MissingCreatedOutputContainer => 13,
            Self::FailedToSetCreatedItemOutputSlot => 14,
            Self::RequestAlreadyInProgress => 15,
            Self::FailedToInitSparseContainer => 16,
            Self::ResultTransferFailed => 17,
            Self::ExpectedItemSlotNotFullyConsumed => 18,
            Self::ExpectedAnywhereItemNotFullyConsumed => 19,
            Self::ItemAlreadyConsumedFromSlot => 20,
            Self::ConsumedTooMuchFromSlot => 21,
            Self::MismatchSlotExpectedConsumedItem => 22,
            Self::MismatchSlotExpectedConsumedItemNetIdVariant => 23,
            Self::FailedToMatchExpectedSlotConsumedItem => 24,
            Self::FailedToMatchExpectedAllowedAnywhereConsumedItem => 25,
            Self::ConsumedItemOutOfAllowedSlotRange => 26,
            Self::ConsumedItemNotAllowed => 27,
            Self::PlayerNotInCreativeMode => 28,
            Self::InvalidExperimentalRecipeRequest => 29,
            Self::FailedToCraftCreative => 30,
            Self::FailedToGetLevelRecipe => 31,
            Self::FailedToFindRecipeByNetId => 32,
            Self::MismatchedCraftingSize => 33,
            Self::MissingInputSparseContainer => 34,
            Self::MismatchedRecipeForInputGridItems => 35,
            Self::EmptyCraftResults => 36,
            Self::FailedToEnchant => 37,
            Self::MissingInputItem => 38,
            Self::InsufficientPlayerLevelToEnchant => 39,
            Self::MissingMaterialItem => 40,
            Self::MissingActor => 41,
            Self::UnknownPrimaryEffect => 42,
            Self::PrimaryEffectOutOfRange => 43,
            Self::PrimaryEffectUnavailable => 44,
            Self::SecondaryEffectOutOfRange => 45,
            Self::SecondaryEffectUnavailable => 46,
            Self::DstContainerEqualToCreatedOutputContainer => 47,
            Self::DstContainerAndSlotEqualToSrcContainerAndSlot => 48,
            Self::FailedToValidateSrcSlot => 49,
            Self::FailedToValidateDstSlot => 50,
            Self::InvalidAdjustedAmount => 51,
            Self::InvalidItemSetType => 52,
            Self::InvalidTransferAmount => 53,
            Self::CannotSwapItem => 54,
            Self::CannotPlaceItem => 55,
            Self::UnhandledItemSetType => 56,
            Self::InvalidRemovedAmount => 57,
            Self::InvalidRegion => 58,
            Self::CannotDropItem => 59,
            Self::CannotDestroyItem => 60,
            Self::InvalidSourceContainer => 61,
            Self::ItemNotConsumed => 62,
            Self::InvalidNumCrafts => 63,
            Self::InvalidCraftResultStackSize => 64,
            Self::CannotRemoveItem => 65,
            Self::CannotConsumeItem => 66,
            Self::ScreenStackError => 67,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemStackNetResult> for u8 {
    fn from(value: ItemStackNetResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemStackNetResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackNetResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ItemStackRequestActionType {
    #[default]
    Take,
    Place,
    Swap,
    Drop,
    Destroy,
    Consume,
    Create,
    PlaceInItemContainer,
    TakeFromItemContainer,
    ScreenLabTableCombine,
    ScreenBeaconPayment,
    ScreenHudMineBlock,
    CraftRecipe,
    CraftRecipeAuto,
    CraftCreative,
    CraftRecipeOptional,
    CraftRepairAndDisenchant,
    CraftLoom,
    CraftReservedAction,
    CraftNonImplemented,
    CraftResults,
    Unknown(u8),
}

impl From<u8> for ItemStackRequestActionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Take,
            1 => Self::Place,
            2 => Self::Swap,
            3 => Self::Drop,
            4 => Self::Destroy,
            5 => Self::Consume,
            6 => Self::Create,
            7 => Self::PlaceInItemContainer,
            8 => Self::TakeFromItemContainer,
            9 => Self::ScreenLabTableCombine,
            10 => Self::ScreenBeaconPayment,
            11 => Self::ScreenHudMineBlock,
            12 => Self::CraftRecipe,
            13 => Self::CraftRecipeAuto,
            14 => Self::CraftCreative,
            15 => Self::CraftRecipeOptional,
            16 => Self::CraftRepairAndDisenchant,
            17 => Self::CraftLoom,
            18 => Self::CraftReservedAction,
            19 => Self::CraftNonImplemented,
            20 => Self::CraftResults,
            value => Self::Unknown(value),
        }
    }
}

impl ItemStackRequestActionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Take => 0,
            Self::Place => 1,
            Self::Swap => 2,
            Self::Drop => 3,
            Self::Destroy => 4,
            Self::Consume => 5,
            Self::Create => 6,
            Self::PlaceInItemContainer => 7,
            Self::TakeFromItemContainer => 8,
            Self::ScreenLabTableCombine => 9,
            Self::ScreenBeaconPayment => 10,
            Self::ScreenHudMineBlock => 11,
            Self::CraftRecipe => 12,
            Self::CraftRecipeAuto => 13,
            Self::CraftCreative => 14,
            Self::CraftRecipeOptional => 15,
            Self::CraftRepairAndDisenchant => 16,
            Self::CraftLoom => 17,
            Self::CraftReservedAction => 18,
            Self::CraftNonImplemented => 19,
            Self::CraftResults => 20,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ItemStackRequestActionType> for u8 {
    fn from(value: ItemStackRequestActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ItemStackRequestActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequestActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: map

/// MapDecoration is a fixed decoration on a map: Its position or other properties do not change automatically
/// client-side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MapDecorationType {
    #[default]
    MarkerWhite,
    MarkerGreen,
    MarkerRed,
    MarkerBlue,
    XWhite,
    TriangleRed,
    SquareWhite,
    MarkerSign,
    MarkerPink,
    MarkerOrange,
    MarkerYellow,
    MarkerTeal,
    TriangleGreen,
    SmallSquareWhite,
    Mansion,
    Monument,
    NoDraw,
    VillageDesert,
    VillagePlains,
    VillageSavanna,
    VillageSnowy,
    VillageTaiga,
    JungleTemple,
    WitchHut,
    TrialChambers,
    AbandonedCamp,
    BuriedAncientCity,
    BuriedMineshaft,
    DesertPyramid,
    WarmOceanRuins,
    Count,
    Unknown(i8),
}

impl From<i8> for MapDecorationType {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::MarkerWhite,
            1 => Self::MarkerGreen,
            2 => Self::MarkerRed,
            3 => Self::MarkerBlue,
            4 => Self::XWhite,
            5 => Self::TriangleRed,
            6 => Self::SquareWhite,
            7 => Self::MarkerSign,
            8 => Self::MarkerPink,
            9 => Self::MarkerOrange,
            10 => Self::MarkerYellow,
            11 => Self::MarkerTeal,
            12 => Self::TriangleGreen,
            13 => Self::SmallSquareWhite,
            14 => Self::Mansion,
            15 => Self::Monument,
            16 => Self::NoDraw,
            17 => Self::VillageDesert,
            18 => Self::VillagePlains,
            19 => Self::VillageSavanna,
            20 => Self::VillageSnowy,
            21 => Self::VillageTaiga,
            22 => Self::JungleTemple,
            23 => Self::WitchHut,
            24 => Self::TrialChambers,
            25 => Self::AbandonedCamp,
            26 => Self::BuriedAncientCity,
            27 => Self::BuriedMineshaft,
            28 => Self::DesertPyramid,
            29 => Self::WarmOceanRuins,
            30 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl MapDecorationType {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::MarkerWhite => 0,
            Self::MarkerGreen => 1,
            Self::MarkerRed => 2,
            Self::MarkerBlue => 3,
            Self::XWhite => 4,
            Self::TriangleRed => 5,
            Self::SquareWhite => 6,
            Self::MarkerSign => 7,
            Self::MarkerPink => 8,
            Self::MarkerOrange => 9,
            Self::MarkerYellow => 10,
            Self::MarkerTeal => 11,
            Self::TriangleGreen => 12,
            Self::SmallSquareWhite => 13,
            Self::Mansion => 14,
            Self::Monument => 15,
            Self::NoDraw => 16,
            Self::VillageDesert => 17,
            Self::VillagePlains => 18,
            Self::VillageSavanna => 19,
            Self::VillageSnowy => 20,
            Self::VillageTaiga => 21,
            Self::JungleTemple => 22,
            Self::WitchHut => 23,
            Self::TrialChambers => 24,
            Self::AbandonedCamp => 25,
            Self::BuriedAncientCity => 26,
            Self::BuriedMineshaft => 27,
            Self::DesertPyramid => 28,
            Self::WarmOceanRuins => 29,
            Self::Count => 30,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MapDecorationType> for i8 {
    fn from(value: MapDecorationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MapDecorationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapDecorationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MapItemTrackedActorType {
    #[default]
    Entity,
    BlockEntity,
    Other,
    Unknown(i32),
}

impl From<i32> for MapItemTrackedActorType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Entity,
            1 => Self::BlockEntity,
            2 => Self::Other,
            value => Self::Unknown(value),
        }
    }
}

impl MapItemTrackedActorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Entity => 0,
            Self::BlockEntity => 1,
            Self::Other => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MapItemTrackedActorType> for i32 {
    fn from(value: MapItemTrackedActorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MapItemTrackedActorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapItemTrackedActorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: memory

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MemoryCategory {
    #[default]
    Unknown,
    InvalidSizeUnknown,
    Actor,
    ActorAnimation,
    ActorRendering,
    BlockTickingQueues,
    BiomeStorage,
    Blobs,
    Cereal,
    CircuitSystem,
    Client,
    Commands,
    DbStorage,
    Debug,
    Documentation,
    EcsSystems,
    Fmod,
    Fonts,
    ImGui,
    Input,
    JsonUi,
    JsonUiControlFactoryJson,
    JsonUiControlTree,
    JsonUiControlTreeControlElement,
    JsonUiControlTreePopulateDataBinding,
    JsonUiControlTreePopulateFocus,
    JsonUiControlTreePopulateLayout,
    JsonUiControlTreePopulateOther,
    JsonUiControlTreePopulateSprite,
    JsonUiControlTreePopulateText,
    JsonUiControlTreePopulateTts,
    JsonUiControlTreeVisibility,
    JsonUiCreateUi,
    JsonUiDefs,
    JsonUiLayoutManager,
    JsonUiLayoutManagerRemoveDependencies,
    JsonUiLayoutManagerInitVariable,
    Languages,
    Level,
    LevelStructures,
    LevelChunk,
    LevelChunkGen,
    LevelChunkGenThreadLocal,
    LightVolumeManager,
    Network,
    Marketplace,
    MaterialDragonCompiledDefinition,
    MaterialDragonMaterial,
    MaterialDragonResource,
    MaterialDragonUniformMap,
    MaterialRenderMaterial,
    MaterialRenderMaterialGroup,
    MaterialVariationManager,
    MoLang,
    OreUi,
    OreUiClient,
    PersonaPieces,
    PersonaAnimations,
    PersonaCharacters,
    PersonaSkinPacks,
    PersonaRepo,
    Player,
    RenderChunk,
    RenderChunkIndexBuffer,
    RenderChunkVertexBuffer,
    Rendering,
    RenderingBgfxInit,
    RenderingBgfxStartFrame,
    RenderingBlockTessellator,
    RenderingEndFrame,
    RenderingGraphicsTasksInit,
    RenderingLibrary,
    RenderingPolygonOperatorPool,
    RenderingPbrTextureData,
    RenderingRenderRegistry,
    RenderingSetup,
    RenderingVertices,
    RequestLog,
    ResourcePacks,
    Sound,
    SubChunkBiomeData,
    SubChunkBlockData,
    SubChunkLightData,
    Textures,
    WeatherRenderer,
    WorldGenerator,
    Tasks,
    Test,
    TestLoadTestTags,
    Scripting,
    ScriptingRuntime,
    ScriptingContext,
    ScriptingContextBindingsMc,
    ScriptingContextBindingsGt,
    ScriptingContextRun,
    DataDrivenUi,
    DataDrivenUiDefs,
    Gameface,
    GamefaceSystem,
    GamefaceDom,
    GamefaceCss,
    GamefaceDisplay,
    GamefaceTempAllocator,
    GamefacePoolAllocator,
    GamefaceDump,
    GamefaceMedia,
    GamefaceJson,
    GamefaceScriptEngine,
    GamefaceScript,
    GamefaceLayout,
    Executable,
    Unknown2(u8),
}

impl From<u8> for MemoryCategory {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::InvalidSizeUnknown,
            2 => Self::Actor,
            3 => Self::ActorAnimation,
            4 => Self::ActorRendering,
            5 => Self::BlockTickingQueues,
            6 => Self::BiomeStorage,
            7 => Self::Blobs,
            8 => Self::Cereal,
            9 => Self::CircuitSystem,
            10 => Self::Client,
            11 => Self::Commands,
            12 => Self::DbStorage,
            13 => Self::Debug,
            14 => Self::Documentation,
            15 => Self::EcsSystems,
            16 => Self::Fmod,
            17 => Self::Fonts,
            18 => Self::ImGui,
            19 => Self::Input,
            20 => Self::JsonUi,
            21 => Self::JsonUiControlFactoryJson,
            22 => Self::JsonUiControlTree,
            23 => Self::JsonUiControlTreeControlElement,
            24 => Self::JsonUiControlTreePopulateDataBinding,
            25 => Self::JsonUiControlTreePopulateFocus,
            26 => Self::JsonUiControlTreePopulateLayout,
            27 => Self::JsonUiControlTreePopulateOther,
            28 => Self::JsonUiControlTreePopulateSprite,
            29 => Self::JsonUiControlTreePopulateText,
            30 => Self::JsonUiControlTreePopulateTts,
            31 => Self::JsonUiControlTreeVisibility,
            32 => Self::JsonUiCreateUi,
            33 => Self::JsonUiDefs,
            34 => Self::JsonUiLayoutManager,
            35 => Self::JsonUiLayoutManagerRemoveDependencies,
            36 => Self::JsonUiLayoutManagerInitVariable,
            37 => Self::Languages,
            38 => Self::Level,
            39 => Self::LevelStructures,
            40 => Self::LevelChunk,
            41 => Self::LevelChunkGen,
            42 => Self::LevelChunkGenThreadLocal,
            43 => Self::LightVolumeManager,
            44 => Self::Network,
            45 => Self::Marketplace,
            46 => Self::MaterialDragonCompiledDefinition,
            47 => Self::MaterialDragonMaterial,
            48 => Self::MaterialDragonResource,
            49 => Self::MaterialDragonUniformMap,
            50 => Self::MaterialRenderMaterial,
            51 => Self::MaterialRenderMaterialGroup,
            52 => Self::MaterialVariationManager,
            53 => Self::MoLang,
            54 => Self::OreUi,
            55 => Self::OreUiClient,
            56 => Self::PersonaPieces,
            57 => Self::PersonaAnimations,
            58 => Self::PersonaCharacters,
            59 => Self::PersonaSkinPacks,
            60 => Self::PersonaRepo,
            61 => Self::Player,
            62 => Self::RenderChunk,
            63 => Self::RenderChunkIndexBuffer,
            64 => Self::RenderChunkVertexBuffer,
            65 => Self::Rendering,
            66 => Self::RenderingBgfxInit,
            67 => Self::RenderingBgfxStartFrame,
            68 => Self::RenderingBlockTessellator,
            69 => Self::RenderingEndFrame,
            70 => Self::RenderingGraphicsTasksInit,
            71 => Self::RenderingLibrary,
            72 => Self::RenderingPolygonOperatorPool,
            73 => Self::RenderingPbrTextureData,
            74 => Self::RenderingRenderRegistry,
            75 => Self::RenderingSetup,
            76 => Self::RenderingVertices,
            77 => Self::RequestLog,
            78 => Self::ResourcePacks,
            79 => Self::Sound,
            80 => Self::SubChunkBiomeData,
            81 => Self::SubChunkBlockData,
            82 => Self::SubChunkLightData,
            83 => Self::Textures,
            84 => Self::WeatherRenderer,
            85 => Self::WorldGenerator,
            86 => Self::Tasks,
            87 => Self::Test,
            88 => Self::TestLoadTestTags,
            89 => Self::Scripting,
            90 => Self::ScriptingRuntime,
            91 => Self::ScriptingContext,
            92 => Self::ScriptingContextBindingsMc,
            93 => Self::ScriptingContextBindingsGt,
            94 => Self::ScriptingContextRun,
            95 => Self::DataDrivenUi,
            96 => Self::DataDrivenUiDefs,
            97 => Self::Gameface,
            98 => Self::GamefaceSystem,
            99 => Self::GamefaceDom,
            100 => Self::GamefaceCss,
            101 => Self::GamefaceDisplay,
            102 => Self::GamefaceTempAllocator,
            103 => Self::GamefacePoolAllocator,
            104 => Self::GamefaceDump,
            105 => Self::GamefaceMedia,
            106 => Self::GamefaceJson,
            107 => Self::GamefaceScriptEngine,
            108 => Self::GamefaceScript,
            109 => Self::GamefaceLayout,
            110 => Self::Executable,
            value => Self::Unknown2(value),
        }
    }
}

impl MemoryCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::InvalidSizeUnknown => 1,
            Self::Actor => 2,
            Self::ActorAnimation => 3,
            Self::ActorRendering => 4,
            Self::BlockTickingQueues => 5,
            Self::BiomeStorage => 6,
            Self::Blobs => 7,
            Self::Cereal => 8,
            Self::CircuitSystem => 9,
            Self::Client => 10,
            Self::Commands => 11,
            Self::DbStorage => 12,
            Self::Debug => 13,
            Self::Documentation => 14,
            Self::EcsSystems => 15,
            Self::Fmod => 16,
            Self::Fonts => 17,
            Self::ImGui => 18,
            Self::Input => 19,
            Self::JsonUi => 20,
            Self::JsonUiControlFactoryJson => 21,
            Self::JsonUiControlTree => 22,
            Self::JsonUiControlTreeControlElement => 23,
            Self::JsonUiControlTreePopulateDataBinding => 24,
            Self::JsonUiControlTreePopulateFocus => 25,
            Self::JsonUiControlTreePopulateLayout => 26,
            Self::JsonUiControlTreePopulateOther => 27,
            Self::JsonUiControlTreePopulateSprite => 28,
            Self::JsonUiControlTreePopulateText => 29,
            Self::JsonUiControlTreePopulateTts => 30,
            Self::JsonUiControlTreeVisibility => 31,
            Self::JsonUiCreateUi => 32,
            Self::JsonUiDefs => 33,
            Self::JsonUiLayoutManager => 34,
            Self::JsonUiLayoutManagerRemoveDependencies => 35,
            Self::JsonUiLayoutManagerInitVariable => 36,
            Self::Languages => 37,
            Self::Level => 38,
            Self::LevelStructures => 39,
            Self::LevelChunk => 40,
            Self::LevelChunkGen => 41,
            Self::LevelChunkGenThreadLocal => 42,
            Self::LightVolumeManager => 43,
            Self::Network => 44,
            Self::Marketplace => 45,
            Self::MaterialDragonCompiledDefinition => 46,
            Self::MaterialDragonMaterial => 47,
            Self::MaterialDragonResource => 48,
            Self::MaterialDragonUniformMap => 49,
            Self::MaterialRenderMaterial => 50,
            Self::MaterialRenderMaterialGroup => 51,
            Self::MaterialVariationManager => 52,
            Self::MoLang => 53,
            Self::OreUi => 54,
            Self::OreUiClient => 55,
            Self::PersonaPieces => 56,
            Self::PersonaAnimations => 57,
            Self::PersonaCharacters => 58,
            Self::PersonaSkinPacks => 59,
            Self::PersonaRepo => 60,
            Self::Player => 61,
            Self::RenderChunk => 62,
            Self::RenderChunkIndexBuffer => 63,
            Self::RenderChunkVertexBuffer => 64,
            Self::Rendering => 65,
            Self::RenderingBgfxInit => 66,
            Self::RenderingBgfxStartFrame => 67,
            Self::RenderingBlockTessellator => 68,
            Self::RenderingEndFrame => 69,
            Self::RenderingGraphicsTasksInit => 70,
            Self::RenderingLibrary => 71,
            Self::RenderingPolygonOperatorPool => 72,
            Self::RenderingPbrTextureData => 73,
            Self::RenderingRenderRegistry => 74,
            Self::RenderingSetup => 75,
            Self::RenderingVertices => 76,
            Self::RequestLog => 77,
            Self::ResourcePacks => 78,
            Self::Sound => 79,
            Self::SubChunkBiomeData => 80,
            Self::SubChunkBlockData => 81,
            Self::SubChunkLightData => 82,
            Self::Textures => 83,
            Self::WeatherRenderer => 84,
            Self::WorldGenerator => 85,
            Self::Tasks => 86,
            Self::Test => 87,
            Self::TestLoadTestTags => 88,
            Self::Scripting => 89,
            Self::ScriptingRuntime => 90,
            Self::ScriptingContext => 91,
            Self::ScriptingContextBindingsMc => 92,
            Self::ScriptingContextBindingsGt => 93,
            Self::ScriptingContextRun => 94,
            Self::DataDrivenUi => 95,
            Self::DataDrivenUiDefs => 96,
            Self::Gameface => 97,
            Self::GamefaceSystem => 98,
            Self::GamefaceDom => 99,
            Self::GamefaceCss => 100,
            Self::GamefaceDisplay => 101,
            Self::GamefaceTempAllocator => 102,
            Self::GamefacePoolAllocator => 103,
            Self::GamefaceDump => 104,
            Self::GamefaceMedia => 105,
            Self::GamefaceJson => 106,
            Self::GamefaceScriptEngine => 107,
            Self::GamefaceScript => 108,
            Self::GamefaceLayout => 109,
            Self::Executable => 110,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<MemoryCategory> for u8 {
    fn from(value: MemoryCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MemoryCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MemoryCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: misc

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AgentActionType {
    #[default]
    Attack,
    Collect,
    Destroy,
    DetectRedstone,
    DetectObstacle,
    Drop,
    DropAll,
    Inspect,
    InspectData,
    InspectItemCount,
    InspectItemDetail,
    InspectItemSpace,
    Interact,
    Move,
    PlaceBlock,
    Till,
    TransferItemTo,
    Turn,
    Unknown(i32),
}

impl From<i32> for AgentActionType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Attack,
            2 => Self::Collect,
            3 => Self::Destroy,
            4 => Self::DetectRedstone,
            5 => Self::DetectObstacle,
            6 => Self::Drop,
            7 => Self::DropAll,
            8 => Self::Inspect,
            9 => Self::InspectData,
            10 => Self::InspectItemCount,
            11 => Self::InspectItemDetail,
            12 => Self::InspectItemSpace,
            13 => Self::Interact,
            14 => Self::Move,
            15 => Self::PlaceBlock,
            16 => Self::Till,
            17 => Self::TransferItemTo,
            18 => Self::Turn,
            value => Self::Unknown(value),
        }
    }
}

impl AgentActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Attack => 1,
            Self::Collect => 2,
            Self::Destroy => 3,
            Self::DetectRedstone => 4,
            Self::DetectObstacle => 5,
            Self::Drop => 6,
            Self::DropAll => 7,
            Self::Inspect => 8,
            Self::InspectData => 9,
            Self::InspectItemCount => 10,
            Self::InspectItemDetail => 11,
            Self::InspectItemSpace => 12,
            Self::Interact => 13,
            Self::Move => 14,
            Self::PlaceBlock => 15,
            Self::Till => 16,
            Self::TransferItemTo => 17,
            Self::Turn => 18,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentActionType> for i32 {
    fn from(value: AgentActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AgentActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AgentAnimationType {
    #[default]
    ArmSwing,
    Shrug,
    Unknown(u8),
}

impl From<u8> for AgentAnimationType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::ArmSwing,
            1 => Self::Shrug,
            value => Self::Unknown(value),
        }
    }
}

impl AgentAnimationType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::ArmSwing => 0,
            Self::Shrug => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentAnimationType> for u8 {
    fn from(value: AgentAnimationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AgentAnimationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentAnimationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AnimateAction {
    #[default]
    NoAction,
    Swing,
    WakeUp,
    CriticalHit,
    MagicCriticalHit,
    Unknown(u8),
}

impl From<u8> for AnimateAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::NoAction,
            1 => Self::Swing,
            3 => Self::WakeUp,
            4 => Self::CriticalHit,
            5 => Self::MagicCriticalHit,
            value => Self::Unknown(value),
        }
    }
}

impl AnimateAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::NoAction => 0,
            Self::Swing => 1,
            Self::WakeUp => 3,
            Self::CriticalHit => 4,
            Self::MagicCriticalHit => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AnimateAction> for u8 {
    fn from(value: AnimateAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AnimateAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnimateAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AnimationMode {
    #[default]
    None,
    Layers,
    Blocks,
    Unknown(u8),
}

impl From<u8> for AnimationMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Layers,
            2 => Self::Blocks,
            value => Self::Unknown(value),
        }
    }
}

impl AnimationMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Layers => 1,
            Self::Blocks => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<AnimationMode> for u8 {
    fn from(value: AnimationMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for AnimationMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnimationMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossBarColor {
    #[default]
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    RebeccaPurple,
    White,
    Unknown(u8),
}

impl From<u8> for BossBarColor {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Pink,
            1 => Self::Blue,
            2 => Self::Red,
            3 => Self::Green,
            4 => Self::Yellow,
            5 => Self::Purple,
            6 => Self::RebeccaPurple,
            7 => Self::White,
            value => Self::Unknown(value),
        }
    }
}

impl BossBarColor {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Pink => 0,
            Self::Blue => 1,
            Self::Red => 2,
            Self::Green => 3,
            Self::Yellow => 4,
            Self::Purple => 5,
            Self::RebeccaPurple => 6,
            Self::White => 7,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossBarColor> for u8 {
    fn from(value: BossBarColor) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossBarColor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossBarColor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossBarOverlay {
    #[default]
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
    Unknown(u8),
}

impl From<u8> for BossBarOverlay {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Progress,
            1 => Self::Notched6,
            2 => Self::Notched10,
            3 => Self::Notched12,
            4 => Self::Notched20,
            value => Self::Unknown(value),
        }
    }
}

impl BossBarOverlay {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Progress => 0,
            Self::Notched6 => 1,
            Self::Notched10 => 2,
            Self::Notched12 => 3,
            Self::Notched20 => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossBarOverlay> for u8 {
    fn from(value: BossBarOverlay) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossBarOverlay {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossBarOverlay {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BossEventUpdateType {
    #[default]
    Add,
    PlayerAdded,
    Remove,
    PlayerRemoved,
    UpdatePercent,
    UpdateName,
    UpdateProperties,
    UpdateStyle,
    Query,
    Unknown(u8),
}

impl From<u8> for BossEventUpdateType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::PlayerAdded,
            2 => Self::Remove,
            3 => Self::PlayerRemoved,
            4 => Self::UpdatePercent,
            5 => Self::UpdateName,
            6 => Self::UpdateProperties,
            7 => Self::UpdateStyle,
            8 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl BossEventUpdateType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::PlayerAdded => 1,
            Self::Remove => 2,
            Self::PlayerRemoved => 3,
            Self::UpdatePercent => 4,
            Self::UpdateName => 5,
            Self::UpdateProperties => 6,
            Self::UpdateStyle => 7,
            Self::Query => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<BossEventUpdateType> for u8 {
    fn from(value: BossEventUpdateType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BossEventUpdateType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossEventUpdateType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BuildPlatform {
    #[default]
    Unknown,
    Google,
    IOs,
    Osx,
    Amazon,
    GearVr,
    Uwp,
    Win32,
    Dedicated,
    TvOs,
    Sony,
    Nintendo,
    Xbox,
    WindowsPhone,
    Linux,
    Unknown2(i32),
}

impl From<i32> for BuildPlatform {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            1 => Self::Google,
            2 => Self::IOs,
            3 => Self::Osx,
            4 => Self::Amazon,
            5 => Self::GearVr,
            7 => Self::Uwp,
            8 => Self::Win32,
            9 => Self::Dedicated,
            10 => Self::TvOs,
            11 => Self::Sony,
            12 => Self::Nintendo,
            13 => Self::Xbox,
            14 => Self::WindowsPhone,
            15 => Self::Linux,
            value => Self::Unknown2(value),
        }
    }
}

impl BuildPlatform {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Google => 1,
            Self::IOs => 2,
            Self::Osx => 3,
            Self::Amazon => 4,
            Self::GearVr => 5,
            Self::Uwp => 7,
            Self::Win32 => 8,
            Self::Dedicated => 9,
            Self::TvOs => 10,
            Self::Sony => 11,
            Self::Nintendo => 12,
            Self::Xbox => 13,
            Self::WindowsPhone => 14,
            Self::Linux => 15,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<BuildPlatform> for i32 {
    fn from(value: BuildPlatform) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for BuildPlatform {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BuildPlatform {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ChatRestrictionLevel {
    #[default]
    None,
    Dropped,
    Disabled,
    Unknown(u8),
}

impl From<u8> for ChatRestrictionLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Dropped,
            2 => Self::Disabled,
            value => Self::Unknown(value),
        }
    }
}

impl ChatRestrictionLevel {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Dropped => 1,
            Self::Disabled => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ChatRestrictionLevel> for u8 {
    fn from(value: ChatRestrictionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ChatRestrictionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChatRestrictionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientCameraAimAssistAction {
    #[default]
    SetFromCameraPreset,
    Clear,
    Unknown(u8),
}

impl From<u8> for ClientCameraAimAssistAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::SetFromCameraPreset,
            1 => Self::Clear,
            value => Self::Unknown(value),
        }
    }
}

impl ClientCameraAimAssistAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::SetFromCameraPreset => 0,
            Self::Clear => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientCameraAimAssistAction> for u8 {
    fn from(value: ClientCameraAimAssistAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientCameraAimAssistAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientCameraAimAssistAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientPlayMode {
    #[default]
    Normal,
    Teaser,
    Screen,
    Viewer,
    Reality,
    Placement,
    LivingRoom,
    ExitLevel,
    ExitLevelLivingRoom,
    NumModes,
    Unknown(u32),
}

impl From<u32> for ClientPlayMode {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Teaser,
            2 => Self::Screen,
            3 => Self::Viewer,
            4 => Self::Reality,
            5 => Self::Placement,
            6 => Self::LivingRoom,
            7 => Self::ExitLevel,
            8 => Self::ExitLevelLivingRoom,
            9 => Self::NumModes,
            value => Self::Unknown(value),
        }
    }
}

impl ClientPlayMode {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Teaser => 1,
            Self::Screen => 2,
            Self::Viewer => 3,
            Self::Reality => 4,
            Self::Placement => 5,
            Self::LivingRoom => 6,
            Self::ExitLevel => 7,
            Self::ExitLevelLivingRoom => 8,
            Self::NumModes => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientPlayMode> for u32 {
    fn from(value: ClientPlayMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientPlayMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientPlayMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClientboundTextureShiftAction {
    #[default]
    Invalid,
    Initialize,
    Start,
    SetEnabled,
    Sync,
    Unknown(u8),
}

impl From<u8> for ClientboundTextureShiftAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Initialize,
            2 => Self::Start,
            3 => Self::SetEnabled,
            4 => Self::Sync,
            value => Self::Unknown(value),
        }
    }
}

impl ClientboundTextureShiftAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Initialize => 1,
            Self::Start => 2,
            Self::SetEnabled => 3,
            Self::Sync => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ClientboundTextureShiftAction> for u8 {
    fn from(value: ClientboundTextureShiftAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ClientboundTextureShiftAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundTextureShiftAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderExecutionStateCodeStatus {
    #[default]
    None,
    NotStarted,
    InProgress,
    Paused,
    Error,
    Succeeded,
    Unknown(u8),
}

impl From<u8> for CodeBuilderExecutionStateCodeStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::NotStarted,
            2 => Self::InProgress,
            3 => Self::Paused,
            4 => Self::Error,
            5 => Self::Succeeded,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderExecutionStateCodeStatus {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::NotStarted => 1,
            Self::InProgress => 2,
            Self::Paused => 3,
            Self::Error => 4,
            Self::Succeeded => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderExecutionStateCodeStatus> for u8 {
    fn from(value: CodeBuilderExecutionStateCodeStatus) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderExecutionStateCodeStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderExecutionStateCodeStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderStorageQueryOptionsCategory {
    #[default]
    None,
    CodeStatus,
    Instantiation,
    Unknown(u8),
}

impl From<u8> for CodeBuilderStorageQueryOptionsCategory {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::CodeStatus,
            2 => Self::Instantiation,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderStorageQueryOptionsCategory {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::CodeStatus => 1,
            Self::Instantiation => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderStorageQueryOptionsCategory> for u8 {
    fn from(value: CodeBuilderStorageQueryOptionsCategory) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderStorageQueryOptionsCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderStorageQueryOptionsCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CodeBuilderStorageQueryOptionsOperation {
    #[default]
    None,
    Get,
    Set,
    Reset,
    Unknown(u8),
}

impl From<u8> for CodeBuilderStorageQueryOptionsOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Get,
            2 => Self::Set,
            3 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl CodeBuilderStorageQueryOptionsOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Get => 1,
            Self::Set => 2,
            Self::Reset => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CodeBuilderStorageQueryOptionsOperation> for u8 {
    fn from(value: CodeBuilderStorageQueryOptionsOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CodeBuilderStorageQueryOptionsOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderStorageQueryOptionsOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ConnectionDisconnectFailReason {
    #[default]
    Unknown,
    CantConnectNoInternet,
    NoPermissions,
    UnrecoverableError,
    ThirdPartyBlocked,
    ThirdPartyNoInternet,
    ThirdPartyBadIp,
    ThirdPartyNoServerOrServerLocked,
    VersionMismatch,
    SkinIssue,
    InviteSessionNotFound,
    EduLevelSettingsMissing,
    LocalServerNotFound,
    LegacyDisconnect,
    InternalUserLeaveGameAttempted,
    PlatformLockedSkinsError,
    RealmsWorldUnassigned,
    RealmsServerCantConnect,
    RealmsServerHidden,
    RealmsServerDisabledBeta,
    RealmsServerDisabled,
    CrossPlatformDisabled,
    TestonlyCantConnect,
    SessionNotFound,
    ClientSettingsIncompatibleWithServer,
    ServerFull,
    InvalidPlatformSkin,
    EditionVersionMismatch,
    EditionMismatch,
    LevelNewerThanExeVersion,
    InternalNoFailOccurred,
    BannedSkin,
    Timeout,
    ServerNotFound,
    OutdatedServer,
    OutdatedClient,
    NoPremiumPlatform,
    MultiplayerDisabled,
    NoWiFi,
    WorldCorruption,
    NoReason,
    Disconnected,
    InvalidPlayer,
    LoggedInOtherLocation,
    ServerIdConflict,
    NotAllowed,
    NotAuthenticated,
    InvalidTenant,
    UnknownPacket,
    UnexpectedPacket,
    InvalidCommandRequestPacket,
    HostSuspended,
    LoginPacketNoRequest,
    LoginPacketNoCert,
    MissingClient,
    Kicked,
    KickedForExploit,
    KickedForIdle,
    ResourcePackProblem,
    IncompatiblePack,
    OutOfStorage,
    InvalidLevel,
    DisconnectPacket,
    BlockMismatch,
    InvalidHeights,
    InvalidWidths,
    ConnectionLost,
    ZombieConnection,
    Shutdown,
    ReasonNotSet,
    LoadingStateTimeout,
    ResourcePackLoadingFailed,
    SearchingForSessionLoadingScreenFailed,
    NetherNetProtocolVersion,
    SubsystemStatusError,
    EmptyAuthFromDiscovery,
    EmptyUrlFromDiscovery,
    ExpiredAuthFromDiscovery,
    UnknownSignalServiceSignInFailure,
    XblJoinLobbyFailure,
    UnspecifiedClientInstanceDisconnection,
    NetherNetSessionNotFound,
    NetherNetCreatePeerConnection,
    NetherNetIce,
    NetherNetConnectRequest,
    NetherNetConnectResponse,
    NetherNetNegotiationTimeout,
    NetherNetInactivityTimeout,
    StaleConnectionBeingReplaced,
    RealmsSessionNotFound,
    BadPacket,
    NetherNetFailedToCreateOffer,
    NetherNetFailedToCreateAnswer,
    NetherNetFailedToSetLocalDescription,
    NetherNetFailedToSetRemoteDescription,
    NetherNetNegotiationTimeoutWaitingForResponse,
    NetherNetNegotiationTimeoutWaitingForAccept,
    NetherNetIncomingConnectionIgnored,
    NetherNetSignalingParsingFailure,
    NetherNetSignalingUnknownError,
    NetherNetSignalingUnicastDeliveryFailed,
    NetherNetSignalingBroadcastDeliveryFailed,
    NetherNetSignalingGenericDeliveryFailed,
    EditorMismatchEditorWorld,
    EditorMismatchVanillaWorld,
    WorldTransferNotPrimaryClient,
    InternalRequestServerShutdown,
    ClientGameSetupCancelled,
    ClientGameSetupFailed,
    NoVenue,
    NetherNetSignalingSigninFailed,
    SessionAccessDenied,
    ServiceSigninIssue,
    NetherNetNoSignalingChannel,
    NetherNetNotLoggedIn,
    NetherNetClientSignalingError,
    SubClientLoginDisabled,
    DeepLinkTryingToOpenDemoWorldWhileSignedIn,
    AsyncJoinTaskDenied,
    RealmsTimelineRequired,
    GuestWithoutHost,
    FailedToJoinExperience,
    NetherNetDataChannelClosed,
    DiscoveryEnvironmentMismatch,
    HostWithoutKeys,
    HostSignedOut,
    ScriptWatchdogException,
    ScriptMemoryLimitExceeded,
    StorageLowDuringGameplay,
    StorageFullDuringGameplay,
    LevelStorageCorruption,
    EditionMismatchVanillaToEdu,
    EditionMismatchEduToVanilla,
    EditorMismatchEditorToVanilla,
    EditorMismatchVanillaToEditor,
    DenyListed,
    NonceMissing,
    NonceNotFound,
    NonceExpired,
    NonceNotValid,
    HostDisconnected,
    EditorJoinIntentPolicyFailure,
    NetherNetIdentityNotAllowed,
    InvalidName,
    ExpiredToken,
    HostAcceptsNoTypeOfAuth,
    NotAuthenticatedFastFail,
    EditorNotAllowed,
    MissingStructureData,
    UnsupportedTransport,
    SubClientLoginNotBound,
    Unknown2(i32),
}

impl From<i32> for ConnectionDisconnectFailReason {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::CantConnectNoInternet,
            2 => Self::NoPermissions,
            3 => Self::UnrecoverableError,
            4 => Self::ThirdPartyBlocked,
            5 => Self::ThirdPartyNoInternet,
            6 => Self::ThirdPartyBadIp,
            7 => Self::ThirdPartyNoServerOrServerLocked,
            8 => Self::VersionMismatch,
            9 => Self::SkinIssue,
            10 => Self::InviteSessionNotFound,
            11 => Self::EduLevelSettingsMissing,
            12 => Self::LocalServerNotFound,
            13 => Self::LegacyDisconnect,
            14 => Self::InternalUserLeaveGameAttempted,
            15 => Self::PlatformLockedSkinsError,
            16 => Self::RealmsWorldUnassigned,
            17 => Self::RealmsServerCantConnect,
            18 => Self::RealmsServerHidden,
            19 => Self::RealmsServerDisabledBeta,
            20 => Self::RealmsServerDisabled,
            21 => Self::CrossPlatformDisabled,
            22 => Self::TestonlyCantConnect,
            23 => Self::SessionNotFound,
            24 => Self::ClientSettingsIncompatibleWithServer,
            25 => Self::ServerFull,
            26 => Self::InvalidPlatformSkin,
            27 => Self::EditionVersionMismatch,
            28 => Self::EditionMismatch,
            29 => Self::LevelNewerThanExeVersion,
            30 => Self::InternalNoFailOccurred,
            31 => Self::BannedSkin,
            32 => Self::Timeout,
            33 => Self::ServerNotFound,
            34 => Self::OutdatedServer,
            35 => Self::OutdatedClient,
            36 => Self::NoPremiumPlatform,
            37 => Self::MultiplayerDisabled,
            38 => Self::NoWiFi,
            39 => Self::WorldCorruption,
            40 => Self::NoReason,
            41 => Self::Disconnected,
            42 => Self::InvalidPlayer,
            43 => Self::LoggedInOtherLocation,
            44 => Self::ServerIdConflict,
            45 => Self::NotAllowed,
            46 => Self::NotAuthenticated,
            47 => Self::InvalidTenant,
            48 => Self::UnknownPacket,
            49 => Self::UnexpectedPacket,
            50 => Self::InvalidCommandRequestPacket,
            51 => Self::HostSuspended,
            52 => Self::LoginPacketNoRequest,
            53 => Self::LoginPacketNoCert,
            54 => Self::MissingClient,
            55 => Self::Kicked,
            56 => Self::KickedForExploit,
            57 => Self::KickedForIdle,
            58 => Self::ResourcePackProblem,
            59 => Self::IncompatiblePack,
            60 => Self::OutOfStorage,
            61 => Self::InvalidLevel,
            62 => Self::DisconnectPacket,
            63 => Self::BlockMismatch,
            64 => Self::InvalidHeights,
            65 => Self::InvalidWidths,
            66 => Self::ConnectionLost,
            67 => Self::ZombieConnection,
            68 => Self::Shutdown,
            69 => Self::ReasonNotSet,
            70 => Self::LoadingStateTimeout,
            71 => Self::ResourcePackLoadingFailed,
            72 => Self::SearchingForSessionLoadingScreenFailed,
            73 => Self::NetherNetProtocolVersion,
            74 => Self::SubsystemStatusError,
            75 => Self::EmptyAuthFromDiscovery,
            76 => Self::EmptyUrlFromDiscovery,
            77 => Self::ExpiredAuthFromDiscovery,
            78 => Self::UnknownSignalServiceSignInFailure,
            79 => Self::XblJoinLobbyFailure,
            80 => Self::UnspecifiedClientInstanceDisconnection,
            81 => Self::NetherNetSessionNotFound,
            82 => Self::NetherNetCreatePeerConnection,
            83 => Self::NetherNetIce,
            84 => Self::NetherNetConnectRequest,
            85 => Self::NetherNetConnectResponse,
            86 => Self::NetherNetNegotiationTimeout,
            87 => Self::NetherNetInactivityTimeout,
            88 => Self::StaleConnectionBeingReplaced,
            89 => Self::RealmsSessionNotFound,
            90 => Self::BadPacket,
            91 => Self::NetherNetFailedToCreateOffer,
            92 => Self::NetherNetFailedToCreateAnswer,
            93 => Self::NetherNetFailedToSetLocalDescription,
            94 => Self::NetherNetFailedToSetRemoteDescription,
            95 => Self::NetherNetNegotiationTimeoutWaitingForResponse,
            96 => Self::NetherNetNegotiationTimeoutWaitingForAccept,
            97 => Self::NetherNetIncomingConnectionIgnored,
            98 => Self::NetherNetSignalingParsingFailure,
            99 => Self::NetherNetSignalingUnknownError,
            100 => Self::NetherNetSignalingUnicastDeliveryFailed,
            101 => Self::NetherNetSignalingBroadcastDeliveryFailed,
            102 => Self::NetherNetSignalingGenericDeliveryFailed,
            103 => Self::EditorMismatchEditorWorld,
            104 => Self::EditorMismatchVanillaWorld,
            105 => Self::WorldTransferNotPrimaryClient,
            106 => Self::InternalRequestServerShutdown,
            107 => Self::ClientGameSetupCancelled,
            108 => Self::ClientGameSetupFailed,
            109 => Self::NoVenue,
            110 => Self::NetherNetSignalingSigninFailed,
            111 => Self::SessionAccessDenied,
            112 => Self::ServiceSigninIssue,
            113 => Self::NetherNetNoSignalingChannel,
            114 => Self::NetherNetNotLoggedIn,
            115 => Self::NetherNetClientSignalingError,
            116 => Self::SubClientLoginDisabled,
            117 => Self::DeepLinkTryingToOpenDemoWorldWhileSignedIn,
            118 => Self::AsyncJoinTaskDenied,
            119 => Self::RealmsTimelineRequired,
            120 => Self::GuestWithoutHost,
            121 => Self::FailedToJoinExperience,
            122 => Self::NetherNetDataChannelClosed,
            123 => Self::DiscoveryEnvironmentMismatch,
            124 => Self::HostWithoutKeys,
            125 => Self::HostSignedOut,
            126 => Self::ScriptWatchdogException,
            127 => Self::ScriptMemoryLimitExceeded,
            128 => Self::StorageLowDuringGameplay,
            129 => Self::StorageFullDuringGameplay,
            130 => Self::LevelStorageCorruption,
            131 => Self::EditionMismatchVanillaToEdu,
            132 => Self::EditionMismatchEduToVanilla,
            133 => Self::EditorMismatchEditorToVanilla,
            134 => Self::EditorMismatchVanillaToEditor,
            135 => Self::DenyListed,
            136 => Self::NonceMissing,
            137 => Self::NonceNotFound,
            138 => Self::NonceExpired,
            139 => Self::NonceNotValid,
            140 => Self::HostDisconnected,
            141 => Self::EditorJoinIntentPolicyFailure,
            142 => Self::NetherNetIdentityNotAllowed,
            143 => Self::InvalidName,
            144 => Self::ExpiredToken,
            145 => Self::HostAcceptsNoTypeOfAuth,
            146 => Self::NotAuthenticatedFastFail,
            147 => Self::EditorNotAllowed,
            148 => Self::MissingStructureData,
            149 => Self::UnsupportedTransport,
            150 => Self::SubClientLoginNotBound,
            value => Self::Unknown2(value),
        }
    }
}

impl ConnectionDisconnectFailReason {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::CantConnectNoInternet => 1,
            Self::NoPermissions => 2,
            Self::UnrecoverableError => 3,
            Self::ThirdPartyBlocked => 4,
            Self::ThirdPartyNoInternet => 5,
            Self::ThirdPartyBadIp => 6,
            Self::ThirdPartyNoServerOrServerLocked => 7,
            Self::VersionMismatch => 8,
            Self::SkinIssue => 9,
            Self::InviteSessionNotFound => 10,
            Self::EduLevelSettingsMissing => 11,
            Self::LocalServerNotFound => 12,
            Self::LegacyDisconnect => 13,
            Self::InternalUserLeaveGameAttempted => 14,
            Self::PlatformLockedSkinsError => 15,
            Self::RealmsWorldUnassigned => 16,
            Self::RealmsServerCantConnect => 17,
            Self::RealmsServerHidden => 18,
            Self::RealmsServerDisabledBeta => 19,
            Self::RealmsServerDisabled => 20,
            Self::CrossPlatformDisabled => 21,
            Self::TestonlyCantConnect => 22,
            Self::SessionNotFound => 23,
            Self::ClientSettingsIncompatibleWithServer => 24,
            Self::ServerFull => 25,
            Self::InvalidPlatformSkin => 26,
            Self::EditionVersionMismatch => 27,
            Self::EditionMismatch => 28,
            Self::LevelNewerThanExeVersion => 29,
            Self::InternalNoFailOccurred => 30,
            Self::BannedSkin => 31,
            Self::Timeout => 32,
            Self::ServerNotFound => 33,
            Self::OutdatedServer => 34,
            Self::OutdatedClient => 35,
            Self::NoPremiumPlatform => 36,
            Self::MultiplayerDisabled => 37,
            Self::NoWiFi => 38,
            Self::WorldCorruption => 39,
            Self::NoReason => 40,
            Self::Disconnected => 41,
            Self::InvalidPlayer => 42,
            Self::LoggedInOtherLocation => 43,
            Self::ServerIdConflict => 44,
            Self::NotAllowed => 45,
            Self::NotAuthenticated => 46,
            Self::InvalidTenant => 47,
            Self::UnknownPacket => 48,
            Self::UnexpectedPacket => 49,
            Self::InvalidCommandRequestPacket => 50,
            Self::HostSuspended => 51,
            Self::LoginPacketNoRequest => 52,
            Self::LoginPacketNoCert => 53,
            Self::MissingClient => 54,
            Self::Kicked => 55,
            Self::KickedForExploit => 56,
            Self::KickedForIdle => 57,
            Self::ResourcePackProblem => 58,
            Self::IncompatiblePack => 59,
            Self::OutOfStorage => 60,
            Self::InvalidLevel => 61,
            Self::DisconnectPacket => 62,
            Self::BlockMismatch => 63,
            Self::InvalidHeights => 64,
            Self::InvalidWidths => 65,
            Self::ConnectionLost => 66,
            Self::ZombieConnection => 67,
            Self::Shutdown => 68,
            Self::ReasonNotSet => 69,
            Self::LoadingStateTimeout => 70,
            Self::ResourcePackLoadingFailed => 71,
            Self::SearchingForSessionLoadingScreenFailed => 72,
            Self::NetherNetProtocolVersion => 73,
            Self::SubsystemStatusError => 74,
            Self::EmptyAuthFromDiscovery => 75,
            Self::EmptyUrlFromDiscovery => 76,
            Self::ExpiredAuthFromDiscovery => 77,
            Self::UnknownSignalServiceSignInFailure => 78,
            Self::XblJoinLobbyFailure => 79,
            Self::UnspecifiedClientInstanceDisconnection => 80,
            Self::NetherNetSessionNotFound => 81,
            Self::NetherNetCreatePeerConnection => 82,
            Self::NetherNetIce => 83,
            Self::NetherNetConnectRequest => 84,
            Self::NetherNetConnectResponse => 85,
            Self::NetherNetNegotiationTimeout => 86,
            Self::NetherNetInactivityTimeout => 87,
            Self::StaleConnectionBeingReplaced => 88,
            Self::RealmsSessionNotFound => 89,
            Self::BadPacket => 90,
            Self::NetherNetFailedToCreateOffer => 91,
            Self::NetherNetFailedToCreateAnswer => 92,
            Self::NetherNetFailedToSetLocalDescription => 93,
            Self::NetherNetFailedToSetRemoteDescription => 94,
            Self::NetherNetNegotiationTimeoutWaitingForResponse => 95,
            Self::NetherNetNegotiationTimeoutWaitingForAccept => 96,
            Self::NetherNetIncomingConnectionIgnored => 97,
            Self::NetherNetSignalingParsingFailure => 98,
            Self::NetherNetSignalingUnknownError => 99,
            Self::NetherNetSignalingUnicastDeliveryFailed => 100,
            Self::NetherNetSignalingBroadcastDeliveryFailed => 101,
            Self::NetherNetSignalingGenericDeliveryFailed => 102,
            Self::EditorMismatchEditorWorld => 103,
            Self::EditorMismatchVanillaWorld => 104,
            Self::WorldTransferNotPrimaryClient => 105,
            Self::InternalRequestServerShutdown => 106,
            Self::ClientGameSetupCancelled => 107,
            Self::ClientGameSetupFailed => 108,
            Self::NoVenue => 109,
            Self::NetherNetSignalingSigninFailed => 110,
            Self::SessionAccessDenied => 111,
            Self::ServiceSigninIssue => 112,
            Self::NetherNetNoSignalingChannel => 113,
            Self::NetherNetNotLoggedIn => 114,
            Self::NetherNetClientSignalingError => 115,
            Self::SubClientLoginDisabled => 116,
            Self::DeepLinkTryingToOpenDemoWorldWhileSignedIn => 117,
            Self::AsyncJoinTaskDenied => 118,
            Self::RealmsTimelineRequired => 119,
            Self::GuestWithoutHost => 120,
            Self::FailedToJoinExperience => 121,
            Self::NetherNetDataChannelClosed => 122,
            Self::DiscoveryEnvironmentMismatch => 123,
            Self::HostWithoutKeys => 124,
            Self::HostSignedOut => 125,
            Self::ScriptWatchdogException => 126,
            Self::ScriptMemoryLimitExceeded => 127,
            Self::StorageLowDuringGameplay => 128,
            Self::StorageFullDuringGameplay => 129,
            Self::LevelStorageCorruption => 130,
            Self::EditionMismatchVanillaToEdu => 131,
            Self::EditionMismatchEduToVanilla => 132,
            Self::EditorMismatchEditorToVanilla => 133,
            Self::EditorMismatchVanillaToEditor => 134,
            Self::DenyListed => 135,
            Self::NonceMissing => 136,
            Self::NonceNotFound => 137,
            Self::NonceExpired => 138,
            Self::NonceNotValid => 139,
            Self::HostDisconnected => 140,
            Self::EditorJoinIntentPolicyFailure => 141,
            Self::NetherNetIdentityNotAllowed => 142,
            Self::InvalidName => 143,
            Self::ExpiredToken => 144,
            Self::HostAcceptsNoTypeOfAuth => 145,
            Self::NotAuthenticatedFastFail => 146,
            Self::EditorNotAllowed => 147,
            Self::MissingStructureData => 148,
            Self::UnsupportedTransport => 149,
            Self::SubClientLoginNotBound => 150,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<ConnectionDisconnectFailReason> for i32 {
    fn from(value: ConnectionDisconnectFailReason) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ConnectionDisconnectFailReason {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ConnectionDisconnectFailReason {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ControlScheme {
    #[default]
    LockedPlayerRelativeStrafe,
    CameraRelative,
    CameraRelativeStrafe,
    PlayerRelative,
    PlayerRelativeStrafe,
    Unknown(u8),
}

impl From<u8> for ControlScheme {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::LockedPlayerRelativeStrafe,
            1 => Self::CameraRelative,
            2 => Self::CameraRelativeStrafe,
            3 => Self::PlayerRelative,
            4 => Self::PlayerRelativeStrafe,
            value => Self::Unknown(value),
        }
    }
}

impl ControlScheme {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::LockedPlayerRelativeStrafe => 0,
            Self::CameraRelative => 1,
            Self::CameraRelativeStrafe => 2,
            Self::PlayerRelative => 3,
            Self::PlayerRelativeStrafe => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ControlScheme> for u8 {
    fn from(value: ControlScheme) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ControlScheme {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ControlScheme {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CoordinateEvaluationOrder {
    #[default]
    Xyz,
    Xzy,
    Yxz,
    Yzx,
    Zxy,
    Zyx,
    Unknown(i32),
}

impl From<i32> for CoordinateEvaluationOrder {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Xyz,
            1 => Self::Xzy,
            2 => Self::Yxz,
            3 => Self::Yzx,
            4 => Self::Zxy,
            5 => Self::Zyx,
            value => Self::Unknown(value),
        }
    }
}

impl CoordinateEvaluationOrder {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Xyz => 0,
            Self::Xzy => 1,
            Self::Yxz => 2,
            Self::Yzx => 3,
            Self::Zxy => 4,
            Self::Zyx => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<CoordinateEvaluationOrder> for i32 {
    fn from(value: CoordinateEvaluationOrder) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for CoordinateEvaluationOrder {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CoordinateEvaluationOrder {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DataItemType {
    #[default]
    Byte,
    Short,
    Int,
    Float,
    String,
    CompoundTag,
    Pos,
    Int64,
    Vec3,
    Unknown(u8),
}

impl From<u8> for DataItemType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Byte,
            1 => Self::Short,
            2 => Self::Int,
            3 => Self::Float,
            4 => Self::String,
            5 => Self::CompoundTag,
            6 => Self::Pos,
            7 => Self::Int64,
            8 => Self::Vec3,
            value => Self::Unknown(value),
        }
    }
}

impl DataItemType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Byte => 0,
            Self::Short => 1,
            Self::Int => 2,
            Self::Float => 3,
            Self::String => 4,
            Self::CompoundTag => 5,
            Self::Pos => 6,
            Self::Int64 => 7,
            Self::Vec3 => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<DataItemType> for u8 {
    fn from(value: DataItemType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for DataItemType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DataItemType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Difficulty {
    #[default]
    Peaceful,
    Easy,
    Normal,
    Hard,
    Count,
    Unknown,
    Unknown2(i32),
}

impl From<i32> for Difficulty {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Peaceful,
            1 => Self::Easy,
            2 => Self::Normal,
            3 => Self::Hard,
            4 => Self::Count,
            5 => Self::Unknown,
            value => Self::Unknown2(value),
        }
    }
}

impl Difficulty {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Peaceful => 0,
            Self::Easy => 1,
            Self::Normal => 2,
            Self::Hard => 3,
            Self::Count => 4,
            Self::Unknown => 5,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<Difficulty> for i32 {
    fn from(value: Difficulty) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Difficulty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Difficulty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GamePublishSetting {
    #[default]
    NoMultiPlay,
    InviteOnly,
    FriendsOnly,
    FriendsOfFriends,
    Public,
    Unknown(i32),
}

impl From<i32> for GamePublishSetting {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::NoMultiPlay,
            1 => Self::InviteOnly,
            2 => Self::FriendsOnly,
            3 => Self::FriendsOfFriends,
            4 => Self::Public,
            value => Self::Unknown(value),
        }
    }
}

impl GamePublishSetting {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::NoMultiPlay => 0,
            Self::InviteOnly => 1,
            Self::FriendsOnly => 2,
            Self::FriendsOfFriends => 3,
            Self::Public => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GamePublishSetting> for i32 {
    fn from(value: GamePublishSetting) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GamePublishSetting {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GamePublishSetting {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GameType {
    #[default]
    Survival,
    Creative,
    Adventure,
    Default,
    Spectator,
    Unknown(i32),
}

impl From<i32> for GameType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Survival,
            1 => Self::Creative,
            2 => Self::Adventure,
            5 => Self::Default,
            6 => Self::Spectator,
            value => Self::Unknown(value),
        }
    }
}

impl GameType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Survival => 0,
            Self::Creative => 1,
            Self::Adventure => 2,
            Self::Default => 5,
            Self::Spectator => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GameType> for i32 {
    fn from(value: GameType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GeneratorType {
    #[default]
    Legacy,
    Overworld,
    Flat,
    Nether,
    TheEnd,
    Void,
    Undefined,
    Unknown(i32),
}

impl From<i32> for GeneratorType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::Overworld,
            2 => Self::Flat,
            3 => Self::Nether,
            4 => Self::TheEnd,
            5 => Self::Void,
            6 => Self::Undefined,
            value => Self::Unknown(value),
        }
    }
}

impl GeneratorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Legacy => 0,
            Self::Overworld => 1,
            Self::Flat => 2,
            Self::Nether => 3,
            Self::TheEnd => 4,
            Self::Void => 5,
            Self::Undefined => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GeneratorType> for i32 {
    fn from(value: GeneratorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GeneratorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GeneratorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GraphicsMode {
    #[default]
    Simple,
    Fancy,
    Advanced,
    RayTraced,
    Unknown(u8),
}

impl From<u8> for GraphicsMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Simple,
            1 => Self::Fancy,
            2 => Self::Advanced,
            3 => Self::RayTraced,
            value => Self::Unknown(value),
        }
    }
}

impl GraphicsMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Simple => 0,
            Self::Fancy => 1,
            Self::Advanced => 2,
            Self::RayTraced => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GraphicsMode> for u8 {
    fn from(value: GraphicsMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GraphicsMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GraphicsMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GraphicsOverrideParameterType {
    #[default]
    SkyZenithColor,
    SkyHorizonColor,
    HorizonBlendMin,
    HorizonBlendMax,
    HorizonBlendStart,
    HorizonBlendMieStart,
    RayleighStrength,
    SunMieStrength,
    MoonMieStrength,
    SunGlareShape,
    Chlorophyll,
    Cdom,
    SuspendedSediment,
    WavesDepth,
    WavesFrequency,
    WavesFrequencyScaling,
    WavesSpeed,
    WavesSpeedScaling,
    WavesShape,
    WavesOctaves,
    WavesMix,
    WavesPull,
    WavesDirectionIncrement,
    MidtonesContrast,
    HighlightsContrast,
    ShadowsContrast,
    HighlightsGain,
    HighlightsGamma,
    HighlightsOffset,
    HighlightsSaturation,
    MidtonesGain,
    MidtonesGamma,
    MidtonesOffset,
    MidtonesSaturation,
    ShadowsGain,
    ShadowsGamma,
    ShadowsOffset,
    ShadowsSaturation,
    HighlightsMin,
    ShadowsMax,
    Temperature,
    SunColor,
    SunIlluminance,
    MoonColor,
    MoonIlluminance,
    FlashColor,
    FlashIlluminance,
    AmbientColor,
    AmbientIlluminance,
    EmissiveDesaturation,
    SkyIntensity,
    OrbitalOffsetDegrees,
    Unknown(u8),
}

impl From<u8> for GraphicsOverrideParameterType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::SkyZenithColor,
            1 => Self::SkyHorizonColor,
            2 => Self::HorizonBlendMin,
            3 => Self::HorizonBlendMax,
            4 => Self::HorizonBlendStart,
            5 => Self::HorizonBlendMieStart,
            6 => Self::RayleighStrength,
            7 => Self::SunMieStrength,
            8 => Self::MoonMieStrength,
            9 => Self::SunGlareShape,
            10 => Self::Chlorophyll,
            11 => Self::Cdom,
            12 => Self::SuspendedSediment,
            13 => Self::WavesDepth,
            14 => Self::WavesFrequency,
            15 => Self::WavesFrequencyScaling,
            16 => Self::WavesSpeed,
            17 => Self::WavesSpeedScaling,
            18 => Self::WavesShape,
            19 => Self::WavesOctaves,
            20 => Self::WavesMix,
            21 => Self::WavesPull,
            22 => Self::WavesDirectionIncrement,
            23 => Self::MidtonesContrast,
            24 => Self::HighlightsContrast,
            25 => Self::ShadowsContrast,
            26 => Self::HighlightsGain,
            27 => Self::HighlightsGamma,
            28 => Self::HighlightsOffset,
            29 => Self::HighlightsSaturation,
            30 => Self::MidtonesGain,
            31 => Self::MidtonesGamma,
            32 => Self::MidtonesOffset,
            33 => Self::MidtonesSaturation,
            34 => Self::ShadowsGain,
            35 => Self::ShadowsGamma,
            36 => Self::ShadowsOffset,
            37 => Self::ShadowsSaturation,
            38 => Self::HighlightsMin,
            39 => Self::ShadowsMax,
            40 => Self::Temperature,
            41 => Self::SunColor,
            42 => Self::SunIlluminance,
            43 => Self::MoonColor,
            44 => Self::MoonIlluminance,
            45 => Self::FlashColor,
            46 => Self::FlashIlluminance,
            47 => Self::AmbientColor,
            48 => Self::AmbientIlluminance,
            49 => Self::EmissiveDesaturation,
            50 => Self::SkyIntensity,
            51 => Self::OrbitalOffsetDegrees,
            value => Self::Unknown(value),
        }
    }
}

impl GraphicsOverrideParameterType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::SkyZenithColor => 0,
            Self::SkyHorizonColor => 1,
            Self::HorizonBlendMin => 2,
            Self::HorizonBlendMax => 3,
            Self::HorizonBlendStart => 4,
            Self::HorizonBlendMieStart => 5,
            Self::RayleighStrength => 6,
            Self::SunMieStrength => 7,
            Self::MoonMieStrength => 8,
            Self::SunGlareShape => 9,
            Self::Chlorophyll => 10,
            Self::Cdom => 11,
            Self::SuspendedSediment => 12,
            Self::WavesDepth => 13,
            Self::WavesFrequency => 14,
            Self::WavesFrequencyScaling => 15,
            Self::WavesSpeed => 16,
            Self::WavesSpeedScaling => 17,
            Self::WavesShape => 18,
            Self::WavesOctaves => 19,
            Self::WavesMix => 20,
            Self::WavesPull => 21,
            Self::WavesDirectionIncrement => 22,
            Self::MidtonesContrast => 23,
            Self::HighlightsContrast => 24,
            Self::ShadowsContrast => 25,
            Self::HighlightsGain => 26,
            Self::HighlightsGamma => 27,
            Self::HighlightsOffset => 28,
            Self::HighlightsSaturation => 29,
            Self::MidtonesGain => 30,
            Self::MidtonesGamma => 31,
            Self::MidtonesOffset => 32,
            Self::MidtonesSaturation => 33,
            Self::ShadowsGain => 34,
            Self::ShadowsGamma => 35,
            Self::ShadowsOffset => 36,
            Self::ShadowsSaturation => 37,
            Self::HighlightsMin => 38,
            Self::ShadowsMax => 39,
            Self::Temperature => 40,
            Self::SunColor => 41,
            Self::SunIlluminance => 42,
            Self::MoonColor => 43,
            Self::MoonIlluminance => 44,
            Self::FlashColor => 45,
            Self::FlashIlluminance => 46,
            Self::AmbientColor => 47,
            Self::AmbientIlluminance => 48,
            Self::EmissiveDesaturation => 49,
            Self::SkyIntensity => 50,
            Self::OrbitalOffsetDegrees => 51,
            Self::Unknown(value) => value,
        }
    }
}

impl From<GraphicsOverrideParameterType> for u8 {
    fn from(value: GraphicsOverrideParameterType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for GraphicsOverrideParameterType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GraphicsOverrideParameterType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HeightMapDataType {
    #[default]
    NoData,
    HasData,
    AllTooHigh,
    AllTooLow,
    AllCopied,
    Unknown(u8),
}

impl From<u8> for HeightMapDataType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::NoData,
            1 => Self::HasData,
            2 => Self::AllTooHigh,
            3 => Self::AllTooLow,
            4 => Self::AllCopied,
            value => Self::Unknown(value),
        }
    }
}

impl HeightMapDataType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::NoData => 0,
            Self::HasData => 1,
            Self::AllTooHigh => 2,
            Self::AllTooLow => 3,
            Self::AllCopied => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HeightMapDataType> for u8 {
    fn from(value: HeightMapDataType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HeightMapDataType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HeightMapDataType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HudElement {
    #[default]
    PaperDoll,
    Armor,
    ToolTips,
    TouchControls,
    Crosshair,
    HotBar,
    Health,
    ProgressBar,
    Hunger,
    AirBubbles,
    HorseHealth,
    StatusEffects,
    ItemText,
    Unknown(i32),
}

impl From<i32> for HudElement {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::PaperDoll,
            1 => Self::Armor,
            2 => Self::ToolTips,
            3 => Self::TouchControls,
            4 => Self::Crosshair,
            5 => Self::HotBar,
            6 => Self::Health,
            7 => Self::ProgressBar,
            8 => Self::Hunger,
            9 => Self::AirBubbles,
            10 => Self::HorseHealth,
            11 => Self::StatusEffects,
            12 => Self::ItemText,
            value => Self::Unknown(value),
        }
    }
}

impl HudElement {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::PaperDoll => 0,
            Self::Armor => 1,
            Self::ToolTips => 2,
            Self::TouchControls => 3,
            Self::Crosshair => 4,
            Self::HotBar => 5,
            Self::Health => 6,
            Self::ProgressBar => 7,
            Self::Hunger => 8,
            Self::AirBubbles => 9,
            Self::HorseHealth => 10,
            Self::StatusEffects => 11,
            Self::ItemText => 12,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HudElement> for i32 {
    fn from(value: HudElement) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HudElement {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HudElement {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HudVisibility {
    #[default]
    Hide,
    Reset,
    Unknown(i32),
}

impl From<i32> for HudVisibility {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Hide,
            1 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl HudVisibility {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Hide => 0,
            Self::Reset => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<HudVisibility> for i32 {
    fn from(value: HudVisibility) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for HudVisibility {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for HudVisibility {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InputData {
    #[default]
    Ascend,
    Descend,
    NorthJump,
    JumpDown,
    SprintDown,
    ChangeHeight,
    Jumping,
    AutoJumpingInWater,
    Sneaking,
    SneakDown,
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    WantUp,
    WantDown,
    WantDownSlow,
    WantUpSlow,
    Sprinting,
    AscendBlock,
    DescendBlock,
    SneakToggleDown,
    PersistSneak,
    StartSprinting,
    StopSprinting,
    StartSneaking,
    StopSneaking,
    StartSwimming,
    StopSwimming,
    StartJumping,
    StartGliding,
    StopGliding,
    PerformItemInteraction,
    PerformBlockActions,
    PerformItemStackRequest,
    HandledTeleport,
    Emoting,
    MissedSwing,
    StartCrawling,
    StopCrawling,
    StartFlying,
    StopFlying,
    ClientAckServerData,
    IsInClientPredictedVehicle,
    PaddlingLeft,
    PaddlingRight,
    BlockBreakingDelayEnabled,
    HorizontalCollision,
    VerticalCollision,
    DownLeft,
    DownRight,
    StartUsingItem,
    IsCameraRelativeMovementEnabled,
    IsRotControlledByMoveDirection,
    StartSpinAttack,
    StopSpinAttack,
    IsHotbarOnlyTouch,
    JumpReleasedRaw,
    JumpPressedRaw,
    JumpCurrentRaw,
    SneakReleasedRaw,
    SneakPressedRaw,
    SneakCurrentRaw,
    InternalUpdate,
    Unknown(i32),
}

impl From<i32> for InputData {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Ascend,
            1 => Self::Descend,
            2 => Self::NorthJump,
            3 => Self::JumpDown,
            4 => Self::SprintDown,
            5 => Self::ChangeHeight,
            6 => Self::Jumping,
            7 => Self::AutoJumpingInWater,
            8 => Self::Sneaking,
            9 => Self::SneakDown,
            10 => Self::Up,
            11 => Self::Down,
            12 => Self::Left,
            13 => Self::Right,
            14 => Self::UpLeft,
            15 => Self::UpRight,
            16 => Self::WantUp,
            17 => Self::WantDown,
            18 => Self::WantDownSlow,
            19 => Self::WantUpSlow,
            20 => Self::Sprinting,
            21 => Self::AscendBlock,
            22 => Self::DescendBlock,
            23 => Self::SneakToggleDown,
            24 => Self::PersistSneak,
            25 => Self::StartSprinting,
            26 => Self::StopSprinting,
            27 => Self::StartSneaking,
            28 => Self::StopSneaking,
            29 => Self::StartSwimming,
            30 => Self::StopSwimming,
            31 => Self::StartJumping,
            32 => Self::StartGliding,
            33 => Self::StopGliding,
            34 => Self::PerformItemInteraction,
            35 => Self::PerformBlockActions,
            36 => Self::PerformItemStackRequest,
            37 => Self::HandledTeleport,
            38 => Self::Emoting,
            39 => Self::MissedSwing,
            40 => Self::StartCrawling,
            41 => Self::StopCrawling,
            42 => Self::StartFlying,
            43 => Self::StopFlying,
            44 => Self::ClientAckServerData,
            45 => Self::IsInClientPredictedVehicle,
            46 => Self::PaddlingLeft,
            47 => Self::PaddlingRight,
            48 => Self::BlockBreakingDelayEnabled,
            49 => Self::HorizontalCollision,
            50 => Self::VerticalCollision,
            51 => Self::DownLeft,
            52 => Self::DownRight,
            53 => Self::StartUsingItem,
            54 => Self::IsCameraRelativeMovementEnabled,
            55 => Self::IsRotControlledByMoveDirection,
            56 => Self::StartSpinAttack,
            57 => Self::StopSpinAttack,
            58 => Self::IsHotbarOnlyTouch,
            59 => Self::JumpReleasedRaw,
            60 => Self::JumpPressedRaw,
            61 => Self::JumpCurrentRaw,
            62 => Self::SneakReleasedRaw,
            63 => Self::SneakPressedRaw,
            64 => Self::SneakCurrentRaw,
            65 => Self::InternalUpdate,
            value => Self::Unknown(value),
        }
    }
}

impl InputData {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Ascend => 0,
            Self::Descend => 1,
            Self::NorthJump => 2,
            Self::JumpDown => 3,
            Self::SprintDown => 4,
            Self::ChangeHeight => 5,
            Self::Jumping => 6,
            Self::AutoJumpingInWater => 7,
            Self::Sneaking => 8,
            Self::SneakDown => 9,
            Self::Up => 10,
            Self::Down => 11,
            Self::Left => 12,
            Self::Right => 13,
            Self::UpLeft => 14,
            Self::UpRight => 15,
            Self::WantUp => 16,
            Self::WantDown => 17,
            Self::WantDownSlow => 18,
            Self::WantUpSlow => 19,
            Self::Sprinting => 20,
            Self::AscendBlock => 21,
            Self::DescendBlock => 22,
            Self::SneakToggleDown => 23,
            Self::PersistSneak => 24,
            Self::StartSprinting => 25,
            Self::StopSprinting => 26,
            Self::StartSneaking => 27,
            Self::StopSneaking => 28,
            Self::StartSwimming => 29,
            Self::StopSwimming => 30,
            Self::StartJumping => 31,
            Self::StartGliding => 32,
            Self::StopGliding => 33,
            Self::PerformItemInteraction => 34,
            Self::PerformBlockActions => 35,
            Self::PerformItemStackRequest => 36,
            Self::HandledTeleport => 37,
            Self::Emoting => 38,
            Self::MissedSwing => 39,
            Self::StartCrawling => 40,
            Self::StopCrawling => 41,
            Self::StartFlying => 42,
            Self::StopFlying => 43,
            Self::ClientAckServerData => 44,
            Self::IsInClientPredictedVehicle => 45,
            Self::PaddlingLeft => 46,
            Self::PaddlingRight => 47,
            Self::BlockBreakingDelayEnabled => 48,
            Self::HorizontalCollision => 49,
            Self::VerticalCollision => 50,
            Self::DownLeft => 51,
            Self::DownRight => 52,
            Self::StartUsingItem => 53,
            Self::IsCameraRelativeMovementEnabled => 54,
            Self::IsRotControlledByMoveDirection => 55,
            Self::StartSpinAttack => 56,
            Self::StopSpinAttack => 57,
            Self::IsHotbarOnlyTouch => 58,
            Self::JumpReleasedRaw => 59,
            Self::JumpPressedRaw => 60,
            Self::JumpCurrentRaw => 61,
            Self::SneakReleasedRaw => 62,
            Self::SneakPressedRaw => 63,
            Self::SneakCurrentRaw => 64,
            Self::InternalUpdate => 65,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InputData> for i32 {
    fn from(value: InputData) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InputData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InputData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InputMode {
    #[default]
    Undefined,
    Mouse,
    Touch,
    GamePad,
    MotionController,
    Count,
    Unknown(u32),
}

impl From<u32> for InputMode {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Undefined,
            1 => Self::Mouse,
            2 => Self::Touch,
            3 => Self::GamePad,
            4 => Self::MotionController,
            5 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl InputMode {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Undefined => 0,
            Self::Mouse => 1,
            Self::Touch => 2,
            Self::GamePad => 3,
            Self::MotionController => 4,
            Self::Count => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InputMode> for u32 {
    fn from(value: InputMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InputMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InputMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum InteractAction {
    #[default]
    Invalid,
    StopRiding,
    InteractUpdate,
    NpcOpen,
    OpenInventory,
    Unknown(u8),
}

impl From<u8> for InteractAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            3 => Self::StopRiding,
            4 => Self::InteractUpdate,
            5 => Self::NpcOpen,
            6 => Self::OpenInventory,
            value => Self::Unknown(value),
        }
    }
}

impl InteractAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::StopRiding => 3,
            Self::InteractUpdate => 4,
            Self::NpcOpen => 5,
            Self::OpenInventory => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<InteractAction> for u8 {
    fn from(value: InteractAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for InteractAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InteractAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LabTableReactionType {
    #[default]
    None,
    IceBomb,
    Bleach,
    ElephantToothpaste,
    Fertilizer,
    HeatBlock,
    MagnesiumSalts,
    MiscFire,
    MiscExplosion,
    MiscLava,
    MiscMystical,
    MiscSmoke,
    MiscLargeSmoke,
    Unknown(u8),
}

impl From<u8> for LabTableReactionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::IceBomb,
            2 => Self::Bleach,
            3 => Self::ElephantToothpaste,
            4 => Self::Fertilizer,
            5 => Self::HeatBlock,
            6 => Self::MagnesiumSalts,
            7 => Self::MiscFire,
            8 => Self::MiscExplosion,
            9 => Self::MiscLava,
            10 => Self::MiscMystical,
            11 => Self::MiscSmoke,
            12 => Self::MiscLargeSmoke,
            value => Self::Unknown(value),
        }
    }
}

impl LabTableReactionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::IceBomb => 1,
            Self::Bleach => 2,
            Self::ElephantToothpaste => 3,
            Self::Fertilizer => 4,
            Self::HeatBlock => 5,
            Self::MagnesiumSalts => 6,
            Self::MiscFire => 7,
            Self::MiscExplosion => 8,
            Self::MiscLava => 9,
            Self::MiscMystical => 10,
            Self::MiscSmoke => 11,
            Self::MiscLargeSmoke => 12,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LabTableReactionType> for u8 {
    fn from(value: LabTableReactionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LabTableReactionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LabTableReactionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LabTableType {
    #[default]
    StartCombine,
    StartReaction,
    Reset,
    Unknown(u8),
}

impl From<u8> for LabTableType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::StartCombine,
            1 => Self::StartReaction,
            2 => Self::Reset,
            value => Self::Unknown(value),
        }
    }
}

impl LabTableType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::StartCombine => 0,
            Self::StartReaction => 1,
            Self::Reset => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LabTableType> for u8 {
    fn from(value: LabTableType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LabTableType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LabTableType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LegacyArmorSlot {
    #[default]
    Head,
    Torso,
    Legs,
    Feet,
    Body,
    Unknown(i32),
}

impl From<i32> for LegacyArmorSlot {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Head,
            1 => Self::Torso,
            2 => Self::Legs,
            3 => Self::Feet,
            4 => Self::Body,
            value => Self::Unknown(value),
        }
    }
}

impl LegacyArmorSlot {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Head => 0,
            Self::Torso => 1,
            Self::Legs => 2,
            Self::Feet => 3,
            Self::Body => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LegacyArmorSlot> for i32 {
    fn from(value: LegacyArmorSlot) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LegacyArmorSlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyArmorSlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LevelGameType {
    #[default]
    Undefined,
    Survival,
    WorldDefault,
    Creative,
    Adventure,
    Default,
    Spectator,
    Unknown(i32),
}

impl From<i32> for LevelGameType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Undefined,
            0 => Self::Survival,
            1 => Self::Creative,
            2 => Self::Adventure,
            5 => Self::Default,
            6 => Self::Spectator,
            value => Self::Unknown(value),
        }
    }
}

impl LevelGameType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Undefined => -1,
            Self::Survival => 0,
            Self::WorldDefault => 0,
            Self::Creative => 1,
            Self::Adventure => 2,
            Self::Default => 5,
            Self::Spectator => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LevelGameType> for i32 {
    fn from(value: LevelGameType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LevelGameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelGameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LevelGeneratorType {
    #[default]
    Legacy,
    Overworld,
    Flat,
    Nether,
    TheEnd,
    Void,
    Undefined,
    Unknown(i32),
}

impl From<i32> for LevelGeneratorType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::Overworld,
            2 => Self::Flat,
            3 => Self::Nether,
            4 => Self::TheEnd,
            5 => Self::Void,
            6 => Self::Undefined,
            value => Self::Unknown(value),
        }
    }
}

impl LevelGeneratorType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Legacy => 0,
            Self::Overworld => 1,
            Self::Flat => 2,
            Self::Nether => 3,
            Self::TheEnd => 4,
            Self::Void => 5,
            Self::Undefined => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LevelGeneratorType> for i32 {
    fn from(value: LevelGeneratorType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LevelGeneratorType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelGeneratorType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingAchievementIds {
    #[default]
    ChestFullOfCobblestone,
    DiamondForYou,
    IronBelly,
    IronMan,
    OnARail,
    Overkill,
    ReturnToSender,
    SniperDuel,
    StayinFrosty,
    TakeInventory,
    MapRoom,
    FreightStation,
    SmeltEverything,
    TasteOfYourOwnMedicine,
    WhenPigsFly,
    Inception,
    ArtificialSelection,
    FreeDiver,
    SpawnTheWither,
    Beaconator,
    GreatView,
    SuperSonic,
    TheEndAgain,
    TreasureHunter,
    ShootingStar,
    FashionShow,
    SelfPublishedAuthor,
    AlternativeFuel,
    SleepWithTheFishes,
    Castaway,
    ImAMarineBiologist,
    SailThe7Seas,
    MeGold,
    Ahoy,
    Atlantis,
    OnePickleTwoPickleSeaPickleFour,
    DoaBarrelRoll,
    Moskstraumen,
    Echolocation,
    WhereHaveYouBeen,
    TopOfTheWorld,
    FruitOnTheLoom,
    SoundTheAlarm,
    BuyLowSellHigh,
    Disenchanted,
    TimeForStew,
    BeeOurGuest,
    TotalBeeLocation,
    StickySituation,
    CoverMeInDebris,
    FloatYourGoat,
    Friend,
    WaxOnWaxOff,
    StriderRiddenInLavaInOverworld,
    GoatHornAcquired,
    JukeboxUsedInMeadows,
    TradedAtWorldHeight,
    SurvivedFallFromWorldHeight,
    SneakCloseToSculkSensor,
    ItSpreads,
    BirthdaySong,
    WithOurPowersCombined,
    PlantingThePast,
    CarefulRestoration,
    Revaulting,
    CraftersCraftingCrafters,
    WhoNeedsRockets,
    OverOverkill,
    HeartTransplanter,
    StayHydrated,
    MobKabob,
    AdventuringTime,
    UhOh,
    GettingWood,
    BenchMaking,
    TimeToMine,
    HotTopic,
    AcquireHardware,
    GettingAnUpgrade,
    MonsterHunter,
    Diamonds,
    PlethoraOfCats,
    WhatGoesAroundComesAround,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingAchievementIds {
    fn from(value: u8) -> Self {
        match value {
            7 => Self::ChestFullOfCobblestone,
            10 => Self::DiamondForYou,
            20 => Self::IronBelly,
            21 => Self::IronMan,
            29 => Self::OnARail,
            30 => Self::Overkill,
            37 => Self::ReturnToSender,
            38 => Self::SniperDuel,
            39 => Self::StayinFrosty,
            40 => Self::TakeInventory,
            50 => Self::MapRoom,
            52 => Self::FreightStation,
            53 => Self::SmeltEverything,
            54 => Self::TasteOfYourOwnMedicine,
            56 => Self::WhenPigsFly,
            58 => Self::Inception,
            60 => Self::ArtificialSelection,
            61 => Self::FreeDiver,
            62 => Self::SpawnTheWither,
            63 => Self::Beaconator,
            64 => Self::GreatView,
            65 => Self::SuperSonic,
            66 => Self::TheEndAgain,
            67 => Self::TreasureHunter,
            68 => Self::ShootingStar,
            69 => Self::FashionShow,
            71 => Self::SelfPublishedAuthor,
            72 => Self::AlternativeFuel,
            73 => Self::SleepWithTheFishes,
            74 => Self::Castaway,
            75 => Self::ImAMarineBiologist,
            76 => Self::SailThe7Seas,
            77 => Self::MeGold,
            78 => Self::Ahoy,
            79 => Self::Atlantis,
            80 => Self::OnePickleTwoPickleSeaPickleFour,
            81 => Self::DoaBarrelRoll,
            82 => Self::Moskstraumen,
            83 => Self::Echolocation,
            84 => Self::WhereHaveYouBeen,
            85 => Self::TopOfTheWorld,
            86 => Self::FruitOnTheLoom,
            87 => Self::SoundTheAlarm,
            88 => Self::BuyLowSellHigh,
            89 => Self::Disenchanted,
            90 => Self::TimeForStew,
            91 => Self::BeeOurGuest,
            92 => Self::TotalBeeLocation,
            93 => Self::StickySituation,
            94 => Self::CoverMeInDebris,
            95 => Self::FloatYourGoat,
            96 => Self::Friend,
            97 => Self::WaxOnWaxOff,
            98 => Self::StriderRiddenInLavaInOverworld,
            99 => Self::GoatHornAcquired,
            100 => Self::JukeboxUsedInMeadows,
            101 => Self::TradedAtWorldHeight,
            102 => Self::SurvivedFallFromWorldHeight,
            103 => Self::SneakCloseToSculkSensor,
            104 => Self::ItSpreads,
            105 => Self::BirthdaySong,
            106 => Self::WithOurPowersCombined,
            107 => Self::PlantingThePast,
            108 => Self::CarefulRestoration,
            109 => Self::Revaulting,
            110 => Self::CraftersCraftingCrafters,
            111 => Self::WhoNeedsRockets,
            112 => Self::OverOverkill,
            113 => Self::HeartTransplanter,
            114 => Self::StayHydrated,
            115 => Self::MobKabob,
            116 => Self::AdventuringTime,
            117 => Self::UhOh,
            118 => Self::GettingWood,
            119 => Self::BenchMaking,
            120 => Self::TimeToMine,
            121 => Self::HotTopic,
            122 => Self::AcquireHardware,
            123 => Self::GettingAnUpgrade,
            124 => Self::MonsterHunter,
            125 => Self::Diamonds,
            126 => Self::PlethoraOfCats,
            127 => Self::WhatGoesAroundComesAround,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingAchievementIds {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::ChestFullOfCobblestone => 7,
            Self::DiamondForYou => 10,
            Self::IronBelly => 20,
            Self::IronMan => 21,
            Self::OnARail => 29,
            Self::Overkill => 30,
            Self::ReturnToSender => 37,
            Self::SniperDuel => 38,
            Self::StayinFrosty => 39,
            Self::TakeInventory => 40,
            Self::MapRoom => 50,
            Self::FreightStation => 52,
            Self::SmeltEverything => 53,
            Self::TasteOfYourOwnMedicine => 54,
            Self::WhenPigsFly => 56,
            Self::Inception => 58,
            Self::ArtificialSelection => 60,
            Self::FreeDiver => 61,
            Self::SpawnTheWither => 62,
            Self::Beaconator => 63,
            Self::GreatView => 64,
            Self::SuperSonic => 65,
            Self::TheEndAgain => 66,
            Self::TreasureHunter => 67,
            Self::ShootingStar => 68,
            Self::FashionShow => 69,
            Self::SelfPublishedAuthor => 71,
            Self::AlternativeFuel => 72,
            Self::SleepWithTheFishes => 73,
            Self::Castaway => 74,
            Self::ImAMarineBiologist => 75,
            Self::SailThe7Seas => 76,
            Self::MeGold => 77,
            Self::Ahoy => 78,
            Self::Atlantis => 79,
            Self::OnePickleTwoPickleSeaPickleFour => 80,
            Self::DoaBarrelRoll => 81,
            Self::Moskstraumen => 82,
            Self::Echolocation => 83,
            Self::WhereHaveYouBeen => 84,
            Self::TopOfTheWorld => 85,
            Self::FruitOnTheLoom => 86,
            Self::SoundTheAlarm => 87,
            Self::BuyLowSellHigh => 88,
            Self::Disenchanted => 89,
            Self::TimeForStew => 90,
            Self::BeeOurGuest => 91,
            Self::TotalBeeLocation => 92,
            Self::StickySituation => 93,
            Self::CoverMeInDebris => 94,
            Self::FloatYourGoat => 95,
            Self::Friend => 96,
            Self::WaxOnWaxOff => 97,
            Self::StriderRiddenInLavaInOverworld => 98,
            Self::GoatHornAcquired => 99,
            Self::JukeboxUsedInMeadows => 100,
            Self::TradedAtWorldHeight => 101,
            Self::SurvivedFallFromWorldHeight => 102,
            Self::SneakCloseToSculkSensor => 103,
            Self::ItSpreads => 104,
            Self::BirthdaySong => 105,
            Self::WithOurPowersCombined => 106,
            Self::PlantingThePast => 107,
            Self::CarefulRestoration => 108,
            Self::Revaulting => 109,
            Self::CraftersCraftingCrafters => 110,
            Self::WhoNeedsRockets => 111,
            Self::OverOverkill => 112,
            Self::HeartTransplanter => 113,
            Self::StayHydrated => 114,
            Self::MobKabob => 115,
            Self::AdventuringTime => 116,
            Self::UhOh => 117,
            Self::GettingWood => 118,
            Self::BenchMaking => 119,
            Self::TimeToMine => 120,
            Self::HotTopic => 121,
            Self::AcquireHardware => 122,
            Self::GettingAnUpgrade => 123,
            Self::MonsterHunter => 124,
            Self::Diamonds => 125,
            Self::PlethoraOfCats => 126,
            Self::WhatGoesAroundComesAround => 127,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingAchievementIds> for u8 {
    fn from(value: MinecraftEventingAchievementIds) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingAchievementIds {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingAchievementIds {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingInteractionType {
    #[default]
    Breeding,
    Taming,
    Curing,
    Crafted,
    Shearing,
    Milking,
    Trading,
    Feeding,
    Igniting,
    Coloring,
    Naming,
    Leashing,
    Unleashing,
    PetSleep,
    Trusting,
    Commanding,
    Equipping,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingInteractionType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Breeding,
            2 => Self::Taming,
            3 => Self::Curing,
            4 => Self::Crafted,
            5 => Self::Shearing,
            6 => Self::Milking,
            7 => Self::Trading,
            8 => Self::Feeding,
            9 => Self::Igniting,
            10 => Self::Coloring,
            11 => Self::Naming,
            12 => Self::Leashing,
            13 => Self::Unleashing,
            14 => Self::PetSleep,
            15 => Self::Trusting,
            16 => Self::Commanding,
            17 => Self::Equipping,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingInteractionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Breeding => 1,
            Self::Taming => 2,
            Self::Curing => 3,
            Self::Crafted => 4,
            Self::Shearing => 5,
            Self::Milking => 6,
            Self::Trading => 7,
            Self::Feeding => 8,
            Self::Igniting => 9,
            Self::Coloring => 10,
            Self::Naming => 11,
            Self::Leashing => 12,
            Self::Unleashing => 13,
            Self::PetSleep => 14,
            Self::Trusting => 15,
            Self::Commanding => 16,
            Self::Equipping => 17,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingInteractionType> for u8 {
    fn from(value: MinecraftEventingInteractionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingInteractionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingInteractionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MinecraftEventingPOIBlockInteractionType {
    #[default]
    None,
    Extend,
    Clone,
    Lock,
    Create,
    CreateLocator,
    Rename,
    ItemPlaced,
    ItemRemoved,
    Cooking,
    Dousing,
    Lighting,
    Haystack,
    Filled,
    Emptied,
    AddDye,
    DyeItem,
    ClearItem,
    EnchantArrow,
    CompostItemPlaced,
    RecoveredBonemeal,
    BookPlaced,
    BookOpened,
    Disenchant,
    Repair,
    DisenchantAndRepair,
    Unknown(u8),
}

impl From<u8> for MinecraftEventingPOIBlockInteractionType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Extend,
            2 => Self::Clone,
            3 => Self::Lock,
            4 => Self::Create,
            5 => Self::CreateLocator,
            6 => Self::Rename,
            7 => Self::ItemPlaced,
            8 => Self::ItemRemoved,
            9 => Self::Cooking,
            10 => Self::Dousing,
            11 => Self::Lighting,
            12 => Self::Haystack,
            13 => Self::Filled,
            14 => Self::Emptied,
            15 => Self::AddDye,
            16 => Self::DyeItem,
            17 => Self::ClearItem,
            18 => Self::EnchantArrow,
            19 => Self::CompostItemPlaced,
            20 => Self::RecoveredBonemeal,
            21 => Self::BookPlaced,
            22 => Self::BookOpened,
            23 => Self::Disenchant,
            24 => Self::Repair,
            25 => Self::DisenchantAndRepair,
            value => Self::Unknown(value),
        }
    }
}

impl MinecraftEventingPOIBlockInteractionType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Extend => 1,
            Self::Clone => 2,
            Self::Lock => 3,
            Self::Create => 4,
            Self::CreateLocator => 5,
            Self::Rename => 6,
            Self::ItemPlaced => 7,
            Self::ItemRemoved => 8,
            Self::Cooking => 9,
            Self::Dousing => 10,
            Self::Lighting => 11,
            Self::Haystack => 12,
            Self::Filled => 13,
            Self::Emptied => 14,
            Self::AddDye => 15,
            Self::DyeItem => 16,
            Self::ClearItem => 17,
            Self::EnchantArrow => 18,
            Self::CompostItemPlaced => 19,
            Self::RecoveredBonemeal => 20,
            Self::BookPlaced => 21,
            Self::BookOpened => 22,
            Self::Disenchant => 23,
            Self::Repair => 24,
            Self::DisenchantAndRepair => 25,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MinecraftEventingPOIBlockInteractionType> for u8 {
    fn from(value: MinecraftEventingPOIBlockInteractionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MinecraftEventingPOIBlockInteractionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MinecraftEventingPOIBlockInteractionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mirror {
    #[default]
    None,
    X,
    Z,
    Xz,
    Unknown(u8),
}

impl From<u8> for Mirror {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::X,
            2 => Self::Z,
            3 => Self::Xz,
            value => Self::Unknown(value),
        }
    }
}

impl Mirror {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::X => 1,
            Self::Z => 2,
            Self::Xz => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Mirror> for u8 {
    fn from(value: Mirror) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Mirror {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Mirror {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MoLangVersion {
    #[default]
    Invalid,
    BeforeVersioning,
    Initial,
    FixedItemRemainingUseDurationQuery,
    ExpressionErrorMessages,
    UnexpectedOperatorErrors,
    ConditionalOperatorAssociativity,
    ComparisonAndLogicalOperatorPrecedence,
    DivideByNegativeValue,
    FixedCapeFlapAmountQuery,
    QueryBlockPropertyRenamedToState,
    DeprecateOldBlockQueryNames,
    DeprecatedSnifferAndCamelQueries,
    LeafSupportingInFirstSolidBlockBelow,
    Latest,
    NumValidVersions,
    Unknown(i16),
}

impl From<i16> for MoLangVersion {
    fn from(value: i16) -> Self {
        match value {
            -1 => Self::Invalid,
            0 => Self::BeforeVersioning,
            1 => Self::Initial,
            2 => Self::FixedItemRemainingUseDurationQuery,
            3 => Self::ExpressionErrorMessages,
            4 => Self::UnexpectedOperatorErrors,
            5 => Self::ConditionalOperatorAssociativity,
            6 => Self::ComparisonAndLogicalOperatorPrecedence,
            7 => Self::DivideByNegativeValue,
            8 => Self::FixedCapeFlapAmountQuery,
            9 => Self::QueryBlockPropertyRenamedToState,
            10 => Self::DeprecateOldBlockQueryNames,
            11 => Self::DeprecatedSnifferAndCamelQueries,
            12 => Self::LeafSupportingInFirstSolidBlockBelow,
            13 => Self::Latest,
            14 => Self::NumValidVersions,
            value => Self::Unknown(value),
        }
    }
}

impl MoLangVersion {
    pub fn to_raw(self) -> i16 {
        match self {
            Self::Invalid => -1,
            Self::BeforeVersioning => 0,
            Self::Initial => 1,
            Self::FixedItemRemainingUseDurationQuery => 2,
            Self::ExpressionErrorMessages => 3,
            Self::UnexpectedOperatorErrors => 4,
            Self::ConditionalOperatorAssociativity => 5,
            Self::ComparisonAndLogicalOperatorPrecedence => 6,
            Self::DivideByNegativeValue => 7,
            Self::FixedCapeFlapAmountQuery => 8,
            Self::QueryBlockPropertyRenamedToState => 9,
            Self::DeprecateOldBlockQueryNames => 10,
            Self::DeprecatedSnifferAndCamelQueries => 11,
            Self::LeafSupportingInFirstSolidBlockBelow => 12,
            Self::Latest => 13,
            Self::NumValidVersions => 14,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MoLangVersion> for i16 {
    fn from(value: MoLangVersion) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MoLangVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoLangVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MobEffectEvent {
    #[default]
    Invalid,
    Add,
    Update,
    Remove,
    Unknown(u8),
}

impl From<u8> for MobEffectEvent {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Invalid,
            1 => Self::Add,
            2 => Self::Update,
            3 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl MobEffectEvent {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Invalid => 0,
            Self::Add => 1,
            Self::Update => 2,
            Self::Remove => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MobEffectEvent> for u8 {
    fn from(value: MobEffectEvent) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MobEffectEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MobEffectEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ModalFormCancelReason {
    #[default]
    UserClosed,
    UserBusy,
    Unknown(u8),
}

impl From<u8> for ModalFormCancelReason {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::UserClosed,
            1 => Self::UserBusy,
            value => Self::Unknown(value),
        }
    }
}

impl ModalFormCancelReason {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::UserClosed => 0,
            Self::UserBusy => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ModalFormCancelReason> for u8 {
    fn from(value: ModalFormCancelReason) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ModalFormCancelReason {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ModalFormCancelReason {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MovementEffectType {
    #[default]
    GlideBoost,
    DolphinBoost,
    GeyserBoost,
    Unknown(i32),
}

impl From<i32> for MovementEffectType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::GlideBoost,
            1 => Self::DolphinBoost,
            2 => Self::GeyserBoost,
            value => Self::Unknown(value),
        }
    }
}

impl MovementEffectType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::GlideBoost => 0,
            Self::DolphinBoost => 1,
            Self::GeyserBoost => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MovementEffectType> for i32 {
    fn from(value: MovementEffectType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MovementEffectType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MovementEffectType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MultiplayerSettingsType {
    #[default]
    EnableMultiplayer,
    DisableMultiplayer,
    RefreshJoinCode,
    Unknown(i32),
}

impl From<i32> for MultiplayerSettingsType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::EnableMultiplayer,
            1 => Self::DisableMultiplayer,
            2 => Self::RefreshJoinCode,
            value => Self::Unknown(value),
        }
    }
}

impl MultiplayerSettingsType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::EnableMultiplayer => 0,
            Self::DisableMultiplayer => 1,
            Self::RefreshJoinCode => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MultiplayerSettingsType> for i32 {
    fn from(value: MultiplayerSettingsType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MultiplayerSettingsType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MultiplayerSettingsType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NewInteractionModel {
    #[default]
    Touch,
    Crosshair,
    Classic,
    Count,
    Unknown(i32),
}

impl From<i32> for NewInteractionModel {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Touch,
            1 => Self::Crosshair,
            2 => Self::Classic,
            3 => Self::Count,
            value => Self::Unknown(value),
        }
    }
}

impl NewInteractionModel {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Touch => 0,
            Self::Crosshair => 1,
            Self::Classic => 2,
            Self::Count => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<NewInteractionModel> for i32 {
    fn from(value: NewInteractionModel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for NewInteractionModel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NewInteractionModel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketCompressionAlgorithm {
    #[default]
    ZLib,
    Snappy,
    None,
    Unknown(u16),
}

impl From<u16> for PacketCompressionAlgorithm {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::ZLib,
            1 => Self::Snappy,
            65535 => Self::None,
            value => Self::Unknown(value),
        }
    }
}

impl PacketCompressionAlgorithm {
    pub fn to_raw(self) -> u16 {
        match self {
            Self::ZLib => 0,
            Self::Snappy => 1,
            Self::None => 65535,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PacketCompressionAlgorithm> for u16 {
    fn from(value: PacketCompressionAlgorithm) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketCompressionAlgorithm {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketCompressionAlgorithm {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketType {
    #[default]
    Empty,
    InitiallyUnlockedRecipes,
    NewlyUnlockedRecipes,
    RemoveUnlockedRecipes,
    RemoveAllUnlockedRecipes,
    Unknown(u32),
}

impl From<u32> for PacketType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Empty,
            1 => Self::InitiallyUnlockedRecipes,
            2 => Self::NewlyUnlockedRecipes,
            3 => Self::RemoveUnlockedRecipes,
            4 => Self::RemoveAllUnlockedRecipes,
            value => Self::Unknown(value),
        }
    }
}

impl PacketType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::InitiallyUnlockedRecipes => 1,
            Self::NewlyUnlockedRecipes => 2,
            Self::RemoveUnlockedRecipes => 3,
            Self::RemoveAllUnlockedRecipes => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PacketType> for u32 {
    fn from(value: PacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketViolationSeverity {
    #[default]
    Unknown,
    Warning,
    FinalWarning,
    TerminatingConnection,
    Unknown2(i32),
}

impl From<i32> for PacketViolationSeverity {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::Warning,
            1 => Self::FinalWarning,
            2 => Self::TerminatingConnection,
            value => Self::Unknown2(value),
        }
    }
}

impl PacketViolationSeverity {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::Warning => 0,
            Self::FinalWarning => 1,
            Self::TerminatingConnection => 2,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PacketViolationSeverity> for i32 {
    fn from(value: PacketViolationSeverity) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketViolationSeverity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketViolationSeverity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PacketViolationType {
    #[default]
    Unknown,
    PacketMalformed,
    Unknown2(i32),
}

impl From<i32> for PacketViolationType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::PacketMalformed,
            value => Self::Unknown2(value),
        }
    }
}

impl PacketViolationType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::PacketMalformed => 0,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PacketViolationType> for i32 {
    fn from(value: PacketViolationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PacketViolationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketViolationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaAnimatedTextureType {
    #[default]
    None,
    Face,
    Body32x32,
    Body128x128,
    Unknown(u32),
}

impl From<u32> for PersonaAnimatedTextureType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Face,
            2 => Self::Body32x32,
            3 => Self::Body128x128,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaAnimatedTextureType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Face => 1,
            Self::Body32x32 => 2,
            Self::Body128x128 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaAnimatedTextureType> for u32 {
    fn from(value: PersonaAnimatedTextureType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaAnimatedTextureType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaAnimatedTextureType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaAnimationExpression {
    #[default]
    Linear,
    Blinking,
    Unknown(u32),
}

impl From<u32> for PersonaAnimationExpression {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Linear,
            1 => Self::Blinking,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaAnimationExpression {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Linear => 0,
            Self::Blinking => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaAnimationExpression> for u32 {
    fn from(value: PersonaAnimationExpression) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaAnimationExpression {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaAnimationExpression {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaArmSizeType {
    #[default]
    Slim,
    Wide,
    Unknown(u8),
}

impl From<u8> for PersonaArmSizeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Slim,
            1 => Self::Wide,
            value => Self::Unknown(value),
        }
    }
}

impl PersonaArmSizeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Slim => 0,
            Self::Wide => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PersonaArmSizeType> for u8 {
    fn from(value: PersonaArmSizeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaArmSizeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaArmSizeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PhotoType {
    #[default]
    Portfolio,
    PhotoItem,
    Book,
    Unknown(u8),
}

impl From<u8> for PhotoType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Portfolio,
            1 => Self::PhotoItem,
            2 => Self::Book,
            value => Self::Unknown(value),
        }
    }
}

impl PhotoType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Portfolio => 0,
            Self::PhotoItem => 1,
            Self::Book => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PhotoType> for u8 {
    fn from(value: PhotoType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PhotoType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PhotoType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayStatusType {
    #[default]
    LoginSuccess,
    LoginFailedClientOld,
    LoginFailedServerOld,
    PlayerSpawn,
    LoginFailedInvalidTenant,
    LoginFailedEditionMismatchEduToVanilla,
    LoginFailedEditionMismatchVanillaToEdu,
    LoginFailedServerFullSubClient,
    LoginFailedEditorMismatchEditorToVanilla,
    LoginFailedEditorMismatchVanillaToEditor,
    Unknown(i32),
}

impl From<i32> for PlayStatusType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::LoginSuccess,
            1 => Self::LoginFailedClientOld,
            2 => Self::LoginFailedServerOld,
            3 => Self::PlayerSpawn,
            4 => Self::LoginFailedInvalidTenant,
            5 => Self::LoginFailedEditionMismatchEduToVanilla,
            6 => Self::LoginFailedEditionMismatchVanillaToEdu,
            7 => Self::LoginFailedServerFullSubClient,
            8 => Self::LoginFailedEditorMismatchEditorToVanilla,
            9 => Self::LoginFailedEditorMismatchVanillaToEditor,
            value => Self::Unknown(value),
        }
    }
}

impl PlayStatusType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::LoginSuccess => 0,
            Self::LoginFailedClientOld => 1,
            Self::LoginFailedServerOld => 2,
            Self::PlayerSpawn => 3,
            Self::LoginFailedInvalidTenant => 4,
            Self::LoginFailedEditionMismatchEduToVanilla => 5,
            Self::LoginFailedEditionMismatchVanillaToEdu => 6,
            Self::LoginFailedServerFullSubClient => 7,
            Self::LoginFailedEditorMismatchEditorToVanilla => 8,
            Self::LoginFailedEditorMismatchVanillaToEditor => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayStatusType> for i32 {
    fn from(value: PlayStatusType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayStatusType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32BE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayStatusType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32BE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RandomDistributionType {
    #[default]
    SingleValued,
    Uniform,
    Gaussian,
    InverseGaussian,
    FixedGrid,
    JitteredGrid,
    Triangle,
    Unknown(i32),
}

impl From<i32> for RandomDistributionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::SingleValued,
            1 => Self::Uniform,
            2 => Self::Gaussian,
            3 => Self::InverseGaussian,
            4 => Self::FixedGrid,
            5 => Self::JitteredGrid,
            6 => Self::Triangle,
            value => Self::Unknown(value),
        }
    }
}

impl RandomDistributionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::SingleValued => 0,
            Self::Uniform => 1,
            Self::Gaussian => 2,
            Self::InverseGaussian => 3,
            Self::FixedGrid => 4,
            Self::JitteredGrid => 5,
            Self::Triangle => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RandomDistributionType> for i32 {
    fn from(value: RandomDistributionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RandomDistributionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RandomDistributionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RequestAbilityType {
    #[default]
    Unset,
    Bool,
    Float,
    Unknown(u8),
}

impl From<u8> for RequestAbilityType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unset,
            1 => Self::Bool,
            2 => Self::Float,
            value => Self::Unknown(value),
        }
    }
}

impl RequestAbilityType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Unset => 0,
            Self::Bool => 1,
            Self::Float => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RequestAbilityType> for u8 {
    fn from(value: RequestAbilityType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RequestAbilityType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RequestAbilityType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RequestType {
    #[default]
    SetActions,
    ExecuteAction,
    ExecuteClosingCommands,
    SetName,
    SetSkin,
    SetInteractText,
    ExecuteOpeningCommands,
    Unknown(u8),
}

impl From<u8> for RequestType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::SetActions,
            1 => Self::ExecuteAction,
            2 => Self::ExecuteClosingCommands,
            3 => Self::SetName,
            4 => Self::SetSkin,
            5 => Self::SetInteractText,
            6 => Self::ExecuteOpeningCommands,
            value => Self::Unknown(value),
        }
    }
}

impl RequestType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::SetActions => 0,
            Self::ExecuteAction => 1,
            Self::ExecuteClosingCommands => 2,
            Self::SetName => 3,
            Self::SetSkin => 4,
            Self::SetInteractText => 5,
            Self::ExecuteOpeningCommands => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RequestType> for u8 {
    fn from(value: RequestType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RequestType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RequestType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RewindType {
    #[default]
    Player,
    Vehicle,
    Unknown(u8),
}

impl From<u8> for RewindType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Player,
            1 => Self::Vehicle,
            value => Self::Unknown(value),
        }
    }
}

impl RewindType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Player => 0,
            Self::Vehicle => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RewindType> for u8 {
    fn from(value: RewindType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RewindType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RewindType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    None,
    Rotate90,
    Rotate180,
    Rotate270,
    Unknown(u8),
}

impl From<u8> for Rotation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Rotate90,
            2 => Self::Rotate180,
            3 => Self::Rotate270,
            value => Self::Unknown(value),
        }
    }
}

impl Rotation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rotate90 => 1,
            Self::Rotate180 => 2,
            Self::Rotate270 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Rotation> for u8 {
    fn from(value: Rotation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Rotation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Rotation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScriptModuleMinecraftScriptPrimitiveShapeType {
    #[default]
    Line,
    Box,
    Sphere,
    Circle,
    Text,
    Arrow,
    Cylinder,
    Pyramid,
    Ellipsoid,
    Cone,
    Unknown(u8),
}

impl From<u8> for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Line,
            1 => Self::Box,
            2 => Self::Sphere,
            3 => Self::Circle,
            4 => Self::Text,
            5 => Self::Arrow,
            6 => Self::Cylinder,
            7 => Self::Pyramid,
            8 => Self::Ellipsoid,
            9 => Self::Cone,
            value => Self::Unknown(value),
        }
    }
}

impl ScriptModuleMinecraftScriptPrimitiveShapeType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Line => 0,
            Self::Box => 1,
            Self::Sphere => 2,
            Self::Circle => 3,
            Self::Text => 4,
            Self::Arrow => 5,
            Self::Cylinder => 6,
            Self::Pyramid => 7,
            Self::Ellipsoid => 8,
            Self::Cone => 9,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ScriptModuleMinecraftScriptPrimitiveShapeType> for u8 {
    fn from(value: ScriptModuleMinecraftScriptPrimitiveShapeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScriptModuleMinecraftScriptPrimitiveShapeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerEditorConnectionPolicy {
    #[default]
    MatchWorldType,
    EditorOnly,
    VanillaOnly,
    Mixed,
    Unknown(i32),
}

impl From<i32> for ServerEditorConnectionPolicy {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::MatchWorldType,
            1 => Self::EditorOnly,
            2 => Self::VanillaOnly,
            3 => Self::Mixed,
            value => Self::Unknown(value),
        }
    }
}

impl ServerEditorConnectionPolicy {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::MatchWorldType => 0,
            Self::EditorOnly => 1,
            Self::VanillaOnly => 2,
            Self::Mixed => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerEditorConnectionPolicy> for i32 {
    fn from(value: ServerEditorConnectionPolicy) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerEditorConnectionPolicy {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerEditorConnectionPolicy {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerWaypointGroupAction {
    #[default]
    None,
    Add,
    Remove,
    Update,
    Unknown(u8),
}

impl From<u8> for ServerWaypointGroupAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Add,
            2 => Self::Remove,
            3 => Self::Update,
            value => Self::Unknown(value),
        }
    }
}

impl ServerWaypointGroupAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Add => 1,
            Self::Remove => 2,
            Self::Update => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerWaypointGroupAction> for u8 {
    fn from(value: ServerWaypointGroupAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerWaypointGroupAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerWaypointGroupAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ServerboundLoadingScreenType {
    #[default]
    StartLoadingScreen,
    EndLoadingScreen,
    Unknown(i32),
}

impl From<i32> for ServerboundLoadingScreenType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::StartLoadingScreen,
            2 => Self::EndLoadingScreen,
            value => Self::Unknown(value),
        }
    }
}

impl ServerboundLoadingScreenType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::StartLoadingScreen => 1,
            Self::EndLoadingScreen => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ServerboundLoadingScreenType> for i32 {
    fn from(value: ServerboundLoadingScreenType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ServerboundLoadingScreenType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerboundLoadingScreenType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ShowStoreOfferRedirectType {
    #[default]
    MarketplaceOffer,
    DressingRoomOffer,
    ThirdPartyServerPage,
    Unknown(u8),
}

impl From<u8> for ShowStoreOfferRedirectType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::MarketplaceOffer,
            1 => Self::DressingRoomOffer,
            2 => Self::ThirdPartyServerPage,
            value => Self::Unknown(value),
        }
    }
}

impl ShowStoreOfferRedirectType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::MarketplaceOffer => 0,
            Self::DressingRoomOffer => 1,
            Self::ThirdPartyServerPage => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ShowStoreOfferRedirectType> for u8 {
    fn from(value: ShowStoreOfferRedirectType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ShowStoreOfferRedirectType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShowStoreOfferRedirectType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SimulationTypeEnum {
    #[default]
    Game,
    Editor,
    Test,
    Invalid,
    Unknown(u8),
}

impl From<u8> for SimulationTypeEnum {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Game,
            1 => Self::Editor,
            2 => Self::Test,
            3 => Self::Invalid,
            value => Self::Unknown(value),
        }
    }
}

impl SimulationTypeEnum {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Game => 0,
            Self::Editor => 1,
            Self::Test => 2,
            Self::Invalid => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SimulationTypeEnum> for u8 {
    fn from(value: SimulationTypeEnum) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SimulationTypeEnum {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SimulationTypeEnum {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinAnimationMode {
    #[default]
    None,
    Layers,
    Blocks,
    Unknown(u8),
}

impl From<u8> for SkinAnimationMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Layers,
            2 => Self::Blocks,
            value => Self::Unknown(value),
        }
    }
}

impl SkinAnimationMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Layers => 1,
            Self::Blocks => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SkinAnimationMode> for u8 {
    fn from(value: SkinAnimationMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SkinAnimationMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SkinAnimationMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SoftEnumUpdateType {
    #[default]
    Add,
    Remove,
    Replace,
    Unknown(u8),
}

impl From<u8> for SoftEnumUpdateType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Remove,
            2 => Self::Replace,
            value => Self::Unknown(value),
        }
    }
}

impl SoftEnumUpdateType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
            Self::Replace => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SoftEnumUpdateType> for u8 {
    fn from(value: SoftEnumUpdateType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SoftEnumUpdateType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SoftEnumUpdateType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SpawnBiomeType {
    #[default]
    Default,
    UserDefined,
    Unknown(i16),
}

impl From<i16> for SpawnBiomeType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::UserDefined,
            value => Self::Unknown(value),
        }
    }
}

impl SpawnBiomeType {
    pub fn to_raw(self) -> i16 {
        match self {
            Self::Default => 0,
            Self::UserDefined => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SpawnBiomeType> for i16 {
    fn from(value: SpawnBiomeType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SpawnBiomeType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnBiomeType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SpawnPositionType {
    #[default]
    PlayerRespawn,
    WorldSpawn,
    Unknown(i32),
}

impl From<i32> for SpawnPositionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::PlayerRespawn,
            1 => Self::WorldSpawn,
            value => Self::Unknown(value),
        }
    }
}

impl SpawnPositionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::PlayerRespawn => 0,
            Self::WorldSpawn => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SpawnPositionType> for i32 {
    fn from(value: SpawnPositionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SpawnPositionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnPositionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureMirror {
    #[default]
    None,
    X,
    Z,
    Xz,
    Unknown(u8),
}

impl From<u8> for StructureMirror {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::X,
            2 => Self::Z,
            3 => Self::Xz,
            value => Self::Unknown(value),
        }
    }
}

impl StructureMirror {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::X => 1,
            Self::Z => 2,
            Self::Xz => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureMirror> for u8 {
    fn from(value: StructureMirror) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureMirror {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureMirror {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureRotation {
    #[default]
    None,
    Rotate90,
    Clockwise90,
    Rotate180,
    Clockwise180,
    Rotate270,
    CounterClockwise90,
    Unknown(u8),
}

impl From<u8> for StructureRotation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Rotate90,
            2 => Self::Rotate180,
            3 => Self::Rotate270,
            value => Self::Unknown(value),
        }
    }
}

impl StructureRotation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rotate90 => 1,
            Self::Clockwise90 => 1,
            Self::Rotate180 => 2,
            Self::Clockwise180 => 2,
            Self::Rotate270 => 3,
            Self::CounterClockwise90 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureRotation> for u8 {
    fn from(value: StructureRotation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureRotation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureRotation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Subtype {
    #[default]
    Uninitialized,
    EnableCommands,
    DisableCommands,
    UnlockWorldTemplateSettings,
    Unknown(u16),
}

impl From<u16> for Subtype {
    fn from(value: u16) -> Self {
        match value {
            0 => Self::Uninitialized,
            1 => Self::EnableCommands,
            2 => Self::DisableCommands,
            3 => Self::UnlockWorldTemplateSettings,
            value => Self::Unknown(value),
        }
    }
}

impl Subtype {
    pub fn to_raw(self) -> u16 {
        match self {
            Self::Uninitialized => 0,
            Self::EnableCommands => 1,
            Self::DisableCommands => 2,
            Self::UnlockWorldTemplateSettings => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<Subtype> for u16 {
    fn from(value: Subtype) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for Subtype {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U16LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Subtype {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U16LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TargetMode {
    #[default]
    Angle,
    Distance,
    Unknown(u8),
}

impl From<u8> for TargetMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Angle,
            1 => Self::Distance,
            value => Self::Unknown(value),
        }
    }
}

impl TargetMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Angle => 0,
            Self::Distance => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TargetMode> for u8 {
    fn from(value: TargetMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TargetMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TargetMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TitleType {
    #[default]
    Clear,
    Reset,
    Title,
    Subtitle,
    Actionbar,
    Times,
    TitleTextObject,
    SubtitleTextObject,
    ActionbarTextObject,
    Unknown(i32),
}

impl From<i32> for TitleType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Clear,
            1 => Self::Reset,
            2 => Self::Title,
            3 => Self::Subtitle,
            4 => Self::Actionbar,
            5 => Self::Times,
            6 => Self::TitleTextObject,
            7 => Self::SubtitleTextObject,
            8 => Self::ActionbarTextObject,
            value => Self::Unknown(value),
        }
    }
}

impl TitleType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Clear => 0,
            Self::Reset => 1,
            Self::Title => 2,
            Self::Subtitle => 3,
            Self::Actionbar => 4,
            Self::Times => 5,
            Self::TitleTextObject => 6,
            Self::SubtitleTextObject => 7,
            Self::ActionbarTextObject => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TitleType> for i32 {
    fn from(value: TitleType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TitleType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TitleType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum VillageType {
    #[default]
    Desert,
    Ice,
    Savanna,
    Taiga,
    Default,
    Unknown(u8),
}

impl From<u8> for VillageType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Desert,
            1 => Self::Ice,
            2 => Self::Savanna,
            3 => Self::Taiga,
            4 => Self::Default,
            value => Self::Unknown(value),
        }
    }
}

impl VillageType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Desert => 0,
            Self::Ice => 1,
            Self::Savanna => 2,
            Self::Taiga => 3,
            Self::Default => 4,
            Self::Unknown(value) => value,
        }
    }
}

impl From<VillageType> for u8 {
    fn from(value: VillageType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for VillageType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for VillageType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: npc

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NpcDialogueActionType {
    #[default]
    Open,
    Close,
    Unknown(i32),
}

impl From<i32> for NpcDialogueActionType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Open,
            1 => Self::Close,
            value => Self::Unknown(value),
        }
    }
}

impl NpcDialogueActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Open => 0,
            Self::Close => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<NpcDialogueActionType> for i32 {
    fn from(value: NpcDialogueActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for NpcDialogueActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NpcDialogueActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: player

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerActionType {
    #[default]
    Unknown,
    StartDestroyBlock,
    AbortDestroyBlock,
    StopDestroyBlock,
    GetUpdatedBlock,
    DropItem,
    StartSleeping,
    StopSleeping,
    Respawn,
    StartJump,
    StartSprinting,
    StopSprinting,
    StartSneaking,
    StopSneaking,
    CreativeDestroyBlock,
    ChangeDimensionAck,
    StartGliding,
    StopGliding,
    DenyDestroyBlock,
    CrackBlock,
    ChangeSkin,
    UpdatedEnchantingSeed,
    StartSwimming,
    StopSwimming,
    StartSpinAttack,
    StopSpinAttack,
    InteractWithBlock,
    PredictDestroyBlock,
    ContinueDestroyBlock,
    StartItemUseOn,
    StopItemUseOn,
    HandledTeleport,
    MissedSwing,
    StartCrawling,
    StopCrawling,
    StartFlying,
    StopFlying,
    ClientAckServerData,
    StartUsingItem,
    InternalUpdate,
    Count,
    Unknown2(i32),
}

impl From<i32> for PlayerActionType {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::StartDestroyBlock,
            1 => Self::AbortDestroyBlock,
            2 => Self::StopDestroyBlock,
            3 => Self::GetUpdatedBlock,
            4 => Self::DropItem,
            5 => Self::StartSleeping,
            6 => Self::StopSleeping,
            7 => Self::Respawn,
            8 => Self::StartJump,
            9 => Self::StartSprinting,
            10 => Self::StopSprinting,
            11 => Self::StartSneaking,
            12 => Self::StopSneaking,
            13 => Self::CreativeDestroyBlock,
            14 => Self::ChangeDimensionAck,
            15 => Self::StartGliding,
            16 => Self::StopGliding,
            17 => Self::DenyDestroyBlock,
            18 => Self::CrackBlock,
            19 => Self::ChangeSkin,
            20 => Self::UpdatedEnchantingSeed,
            21 => Self::StartSwimming,
            22 => Self::StopSwimming,
            23 => Self::StartSpinAttack,
            24 => Self::StopSpinAttack,
            25 => Self::InteractWithBlock,
            26 => Self::PredictDestroyBlock,
            27 => Self::ContinueDestroyBlock,
            28 => Self::StartItemUseOn,
            29 => Self::StopItemUseOn,
            30 => Self::HandledTeleport,
            31 => Self::MissedSwing,
            32 => Self::StartCrawling,
            33 => Self::StopCrawling,
            34 => Self::StartFlying,
            35 => Self::StopFlying,
            36 => Self::ClientAckServerData,
            37 => Self::StartUsingItem,
            38 => Self::InternalUpdate,
            39 => Self::Count,
            value => Self::Unknown2(value),
        }
    }
}

impl PlayerActionType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::StartDestroyBlock => 0,
            Self::AbortDestroyBlock => 1,
            Self::StopDestroyBlock => 2,
            Self::GetUpdatedBlock => 3,
            Self::DropItem => 4,
            Self::StartSleeping => 5,
            Self::StopSleeping => 6,
            Self::Respawn => 7,
            Self::StartJump => 8,
            Self::StartSprinting => 9,
            Self::StopSprinting => 10,
            Self::StartSneaking => 11,
            Self::StopSneaking => 12,
            Self::CreativeDestroyBlock => 13,
            Self::ChangeDimensionAck => 14,
            Self::StartGliding => 15,
            Self::StopGliding => 16,
            Self::DenyDestroyBlock => 17,
            Self::CrackBlock => 18,
            Self::ChangeSkin => 19,
            Self::UpdatedEnchantingSeed => 20,
            Self::StartSwimming => 21,
            Self::StopSwimming => 22,
            Self::StartSpinAttack => 23,
            Self::StopSpinAttack => 24,
            Self::InteractWithBlock => 25,
            Self::PredictDestroyBlock => 26,
            Self::ContinueDestroyBlock => 27,
            Self::StartItemUseOn => 28,
            Self::StopItemUseOn => 29,
            Self::HandledTeleport => 30,
            Self::MissedSwing => 31,
            Self::StartCrawling => 32,
            Self::StopCrawling => 33,
            Self::StartFlying => 34,
            Self::StopFlying => 35,
            Self::ClientAckServerData => 36,
            Self::StartUsingItem => 37,
            Self::InternalUpdate => 38,
            Self::Count => 39,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PlayerActionType> for i32 {
    fn from(value: PlayerActionType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerActionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerActionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerListPacketType {
    #[default]
    Add,
    Remove,
    Unknown(u8),
}

impl From<u8> for PlayerListPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Add,
            1 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerListPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerListPacketType> for u8 {
    fn from(value: PlayerListPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerListPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerListPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerLocationType {
    #[default]
    PlayerLocationCoordinates,
    PlayerLocationHide,
    Unknown(i32),
}

impl From<i32> for PlayerLocationType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::PlayerLocationCoordinates,
            1 => Self::PlayerLocationHide,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerLocationType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::PlayerLocationCoordinates => 0,
            Self::PlayerLocationHide => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerLocationType> for i32 {
    fn from(value: PlayerLocationType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerLocationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerLocationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerPermission {
    #[default]
    Visitor,
    Member,
    Operator,
    Custom,
    Unknown(i8),
}

impl From<i8> for PlayerPermission {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::Visitor,
            1 => Self::Member,
            2 => Self::Operator,
            3 => Self::Custom,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerPermission {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::Visitor => 0,
            Self::Member => 1,
            Self::Operator => 2,
            Self::Custom => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerPermission> for i8 {
    fn from(value: PlayerPermission) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerPermission {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPermission {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerPermissionLevel {
    #[default]
    Visitor,
    Member,
    Operator,
    Custom,
    Unknown(i8),
}

impl From<i8> for PlayerPermissionLevel {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::Visitor,
            1 => Self::Member,
            2 => Self::Operator,
            3 => Self::Custom,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerPermissionLevel {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::Visitor => 0,
            Self::Member => 1,
            Self::Operator => 2,
            Self::Custom => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerPermissionLevel> for i8 {
    fn from(value: PlayerPermissionLevel) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerPermissionLevel {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPermissionLevel {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerPositionModeComponentPositionMode {
    #[default]
    Normal,
    Respawn,
    Teleport,
    OnlyHeadRot,
    Unknown(u8),
}

impl From<u8> for PlayerPositionModeComponentPositionMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Respawn,
            2 => Self::Teleport,
            3 => Self::OnlyHeadRot,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerPositionModeComponentPositionMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Respawn => 1,
            Self::Teleport => 2,
            Self::OnlyHeadRot => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerPositionModeComponentPositionMode> for u8 {
    fn from(value: PlayerPositionModeComponentPositionMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerPositionModeComponentPositionMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPositionModeComponentPositionMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PlayerRespawnState {
    #[default]
    SearchingForSpawn,
    ReadyToSpawn,
    ClientReadyToSpawn,
    Unknown(u8),
}

impl From<u8> for PlayerRespawnState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::SearchingForSpawn,
            1 => Self::ReadyToSpawn,
            2 => Self::ClientReadyToSpawn,
            value => Self::Unknown(value),
        }
    }
}

impl PlayerRespawnState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::SearchingForSpawn => 0,
            Self::ReadyToSpawn => 1,
            Self::ClientReadyToSpawn => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PlayerRespawnState> for u8 {
    fn from(value: PlayerRespawnState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PlayerRespawnState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerRespawnState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: position_tracking

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PositionTrackingDBClientRequestAction {
    #[default]
    Query,
    Unknown(u8),
}

impl From<u8> for PositionTrackingDBClientRequestAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl PositionTrackingDBClientRequestAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Query => 0,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PositionTrackingDBClientRequestAction> for u8 {
    fn from(value: PositionTrackingDBClientRequestAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PositionTrackingDBClientRequestAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBClientRequestAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PositionTrackingDBServerBroadcastAction {
    #[default]
    Update,
    Destroy,
    NotFound,
    Unknown(u8),
}

impl From<u8> for PositionTrackingDBServerBroadcastAction {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Update,
            1 => Self::Destroy,
            2 => Self::NotFound,
            value => Self::Unknown(value),
        }
    }
}

impl PositionTrackingDBServerBroadcastAction {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Update => 0,
            Self::Destroy => 1,
            Self::NotFound => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<PositionTrackingDBServerBroadcastAction> for u8 {
    fn from(value: PositionTrackingDBServerBroadcastAction) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PositionTrackingDBServerBroadcastAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBServerBroadcastAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: recipe

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RecipeUnlockingRequirementUnlockingContext {
    #[default]
    None,
    AlwaysUnlocked,
    PlayerInWater,
    PlayerHasManyItems,
    Unknown(i32),
}

impl From<i32> for RecipeUnlockingRequirementUnlockingContext {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::AlwaysUnlocked,
            2 => Self::PlayerInWater,
            3 => Self::PlayerHasManyItems,
            value => Self::Unknown(value),
        }
    }
}

impl RecipeUnlockingRequirementUnlockingContext {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::AlwaysUnlocked => 1,
            Self::PlayerInWater => 2,
            Self::PlayerHasManyItems => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<RecipeUnlockingRequirementUnlockingContext> for i32 {
    fn from(value: RecipeUnlockingRequirementUnlockingContext) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for RecipeUnlockingRequirementUnlockingContext {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RecipeUnlockingRequirementUnlockingContext {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: scoreboard

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScoreboardIdentityPacketType {
    #[default]
    Update,
    Remove,
    Unknown(u8),
}

impl From<u8> for ScoreboardIdentityPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Update,
            1 => Self::Remove,
            value => Self::Unknown(value),
        }
    }
}

impl ScoreboardIdentityPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Update => 0,
            Self::Remove => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<ScoreboardIdentityPacketType> for u8 {
    fn from(value: ScoreboardIdentityPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for ScoreboardIdentityPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScoreboardIdentityPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: server

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MatchmakingState {
    #[default]
    Idle,
    Matchmaking,
    MatchFound,
    Canceled,
    PlayerLeftParty,
    PlayerLeftServer,
    ServerShutdown,
    TimedOut,
    RequeueAsParty,
    Unknown(u8),
}

impl From<u8> for MatchmakingState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::Matchmaking,
            2 => Self::MatchFound,
            3 => Self::Canceled,
            4 => Self::PlayerLeftParty,
            5 => Self::PlayerLeftServer,
            6 => Self::ServerShutdown,
            7 => Self::TimedOut,
            8 => Self::RequeueAsParty,
            value => Self::Unknown(value),
        }
    }
}

impl MatchmakingState {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Matchmaking => 1,
            Self::MatchFound => 2,
            Self::Canceled => 3,
            Self::PlayerLeftParty => 4,
            Self::PlayerLeftServer => 5,
            Self::ServerShutdown => 6,
            Self::TimedOut => 7,
            Self::RequeueAsParty => 8,
            Self::Unknown(value) => value,
        }
    }
}

impl From<MatchmakingState> for u8 {
    fn from(value: MatchmakingState) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for MatchmakingState {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MatchmakingState {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: skin

/// PersonaPiece represents a piece of a persona skin. All pieces are sent separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PersonaPieceType {
    #[default]
    Unknown,
    Skeleton,
    Body,
    Skin,
    Bottom,
    Feet,
    Dress,
    Top,
    HighPants,
    Hands,
    Outerwear,
    FacialHair,
    Mouth,
    Eyes,
    Hair,
    Hood,
    Back,
    FaceAccessory,
    Head,
    Legs,
    LeftLeg,
    RightLeg,
    Arms,
    LeftArm,
    RightArm,
    Capes,
    ClassicSkin,
    Emote,
    CoCo,
    Unsupported,
    Unknown2(u32),
}

impl From<u32> for PersonaPieceType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::Skeleton,
            2 => Self::Body,
            3 => Self::Skin,
            4 => Self::Bottom,
            5 => Self::Feet,
            6 => Self::Dress,
            7 => Self::Top,
            8 => Self::HighPants,
            9 => Self::Hands,
            10 => Self::Outerwear,
            11 => Self::FacialHair,
            12 => Self::Mouth,
            13 => Self::Eyes,
            14 => Self::Hair,
            15 => Self::Hood,
            16 => Self::Back,
            17 => Self::FaceAccessory,
            18 => Self::Head,
            19 => Self::Legs,
            20 => Self::LeftLeg,
            21 => Self::RightLeg,
            22 => Self::Arms,
            23 => Self::LeftArm,
            24 => Self::RightArm,
            25 => Self::Capes,
            26 => Self::ClassicSkin,
            27 => Self::Emote,
            28 => Self::CoCo,
            29 => Self::Unsupported,
            value => Self::Unknown2(value),
        }
    }
}

impl PersonaPieceType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Skeleton => 1,
            Self::Body => 2,
            Self::Skin => 3,
            Self::Bottom => 4,
            Self::Feet => 5,
            Self::Dress => 6,
            Self::Top => 7,
            Self::HighPants => 8,
            Self::Hands => 9,
            Self::Outerwear => 10,
            Self::FacialHair => 11,
            Self::Mouth => 12,
            Self::Eyes => 13,
            Self::Hair => 14,
            Self::Hood => 15,
            Self::Back => 16,
            Self::FaceAccessory => 17,
            Self::Head => 18,
            Self::Legs => 19,
            Self::LeftLeg => 20,
            Self::RightLeg => 21,
            Self::Arms => 22,
            Self::LeftArm => 23,
            Self::RightArm => 24,
            Self::Capes => 25,
            Self::ClassicSkin => 26,
            Self::Emote => 27,
            Self::CoCo => 28,
            Self::Unsupported => 29,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<PersonaPieceType> for u32 {
    fn from(value: PersonaPieceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for PersonaPieceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PersonaPieceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U32LE as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: structure

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureBlockType {
    #[default]
    Data,
    Save,
    Load,
    Corner,
    Invalid,
    Export,
    Unknown(i32),
}

impl From<i32> for StructureBlockType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Data,
            1 => Self::Save,
            2 => Self::Load,
            3 => Self::Corner,
            4 => Self::Invalid,
            5 => Self::Export,
            value => Self::Unknown(value),
        }
    }
}

impl StructureBlockType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Data => 0,
            Self::Save => 1,
            Self::Load => 2,
            Self::Corner => 3,
            Self::Invalid => 4,
            Self::Export => 5,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureBlockType> for i32 {
    fn from(value: StructureBlockType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureBlockType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureBlockType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureRedstoneSaveMode {
    #[default]
    SavesToMemory,
    SavesToDisk,
    Unknown(u8),
}

impl From<u8> for StructureRedstoneSaveMode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::SavesToMemory,
            1 => Self::SavesToDisk,
            value => Self::Unknown(value),
        }
    }
}

impl StructureRedstoneSaveMode {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::SavesToMemory => 0,
            Self::SavesToDisk => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureRedstoneSaveMode> for u8 {
    fn from(value: StructureRedstoneSaveMode) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureRedstoneSaveMode {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureRedstoneSaveMode {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureTemplateRequestOperation {
    #[default]
    None,
    ExportFromSaveMode,
    ExportFromLoadMode,
    QuerySavedStructure,
    Unknown(u8),
}

impl From<u8> for StructureTemplateRequestOperation {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::ExportFromSaveMode,
            2 => Self::ExportFromLoadMode,
            3 => Self::QuerySavedStructure,
            value => Self::Unknown(value),
        }
    }
}

impl StructureTemplateRequestOperation {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::ExportFromSaveMode => 1,
            Self::ExportFromLoadMode => 2,
            Self::QuerySavedStructure => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureTemplateRequestOperation> for u8 {
    fn from(value: StructureTemplateRequestOperation) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureTemplateRequestOperation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateRequestOperation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StructureTemplateResponseType {
    #[default]
    None,
    Export,
    Query,
    Unknown(u8),
}

impl From<u8> for StructureTemplateResponseType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Export,
            2 => Self::Query,
            value => Self::Unknown(value),
        }
    }
}

impl StructureTemplateResponseType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Export => 1,
            Self::Query => 2,
            Self::Unknown(value) => value,
        }
    }
}

impl From<StructureTemplateResponseType> for u8 {
    fn from(value: StructureTemplateResponseType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for StructureTemplateResponseType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateResponseType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: sub_chunk

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SubChunkRequestResult {
    #[default]
    Success,
    LevelChunkDoesntExist,
    WrongDimension,
    PlayerDoesntExist,
    IndexOutOfBounds,
    SuccessAllAir,
    Unknown(u8),
}

impl From<u8> for SubChunkRequestResult {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Success,
            2 => Self::LevelChunkDoesntExist,
            3 => Self::WrongDimension,
            4 => Self::PlayerDoesntExist,
            5 => Self::IndexOutOfBounds,
            6 => Self::SuccessAllAir,
            value => Self::Unknown(value),
        }
    }
}

impl SubChunkRequestResult {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Success => 1,
            Self::LevelChunkDoesntExist => 2,
            Self::WrongDimension => 3,
            Self::PlayerDoesntExist => 4,
            Self::IndexOutOfBounds => 5,
            Self::SuccessAllAir => 6,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SubChunkRequestResult> for u8 {
    fn from(value: SubChunkRequestResult) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SubChunkRequestResult {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkRequestResult {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: telemetry

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LegacyTelemetryType {
    #[default]
    Achievement,
    Interaction,
    PortalCreated,
    PortalUsed,
    MobKilled,
    CauldronUsed,
    PlayerDied,
    BossKilled,
    AgentCommandObsolete,
    AgentCreated,
    PatternRemovedObsolete,
    SlashCommand,
    FishBucketedObsolete,
    MobBorn,
    PetDiedObsolete,
    PoiCauldronUsed,
    ComposterUsed,
    BellUsed,
    ActorDefinition,
    RaidUpdate,
    PlayerMovementAnomalyObsolete,
    PlayerMovementCorrectedObsolete,
    HoneyHarvested,
    TargetBlockHit,
    PiglinBarter,
    PlayerWaxedOrUnwaxedCopper,
    CodeBuilderRuntimeAction,
    CodeBuilderScoreboard,
    StriderRiddenInLavaInOverworld,
    SneakCloseToSculkSensor,
    CarefulRestoration,
    ItemUsed,
    Unknown(i32),
}

impl From<i32> for LegacyTelemetryType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Achievement,
            1 => Self::Interaction,
            2 => Self::PortalCreated,
            3 => Self::PortalUsed,
            4 => Self::MobKilled,
            5 => Self::CauldronUsed,
            6 => Self::PlayerDied,
            7 => Self::BossKilled,
            8 => Self::AgentCommandObsolete,
            9 => Self::AgentCreated,
            10 => Self::PatternRemovedObsolete,
            11 => Self::SlashCommand,
            12 => Self::FishBucketedObsolete,
            13 => Self::MobBorn,
            14 => Self::PetDiedObsolete,
            15 => Self::PoiCauldronUsed,
            16 => Self::ComposterUsed,
            17 => Self::BellUsed,
            18 => Self::ActorDefinition,
            19 => Self::RaidUpdate,
            20 => Self::PlayerMovementAnomalyObsolete,
            21 => Self::PlayerMovementCorrectedObsolete,
            22 => Self::HoneyHarvested,
            23 => Self::TargetBlockHit,
            24 => Self::PiglinBarter,
            25 => Self::PlayerWaxedOrUnwaxedCopper,
            26 => Self::CodeBuilderRuntimeAction,
            27 => Self::CodeBuilderScoreboard,
            28 => Self::StriderRiddenInLavaInOverworld,
            29 => Self::SneakCloseToSculkSensor,
            30 => Self::CarefulRestoration,
            31 => Self::ItemUsed,
            value => Self::Unknown(value),
        }
    }
}

impl LegacyTelemetryType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Achievement => 0,
            Self::Interaction => 1,
            Self::PortalCreated => 2,
            Self::PortalUsed => 3,
            Self::MobKilled => 4,
            Self::CauldronUsed => 5,
            Self::PlayerDied => 6,
            Self::BossKilled => 7,
            Self::AgentCommandObsolete => 8,
            Self::AgentCreated => 9,
            Self::PatternRemovedObsolete => 10,
            Self::SlashCommand => 11,
            Self::FishBucketedObsolete => 12,
            Self::MobBorn => 13,
            Self::PetDiedObsolete => 14,
            Self::PoiCauldronUsed => 15,
            Self::ComposterUsed => 16,
            Self::BellUsed => 17,
            Self::ActorDefinition => 18,
            Self::RaidUpdate => 19,
            Self::PlayerMovementAnomalyObsolete => 20,
            Self::PlayerMovementCorrectedObsolete => 21,
            Self::HoneyHarvested => 22,
            Self::TargetBlockHit => 23,
            Self::PiglinBarter => 24,
            Self::PlayerWaxedOrUnwaxedCopper => 25,
            Self::CodeBuilderRuntimeAction => 26,
            Self::CodeBuilderScoreboard => 27,
            Self::StriderRiddenInLavaInOverworld => 28,
            Self::SneakCloseToSculkSensor => 29,
            Self::CarefulRestoration => 30,
            Self::ItemUsed => 31,
            Self::Unknown(value) => value,
        }
    }
}

impl From<LegacyTelemetryType> for i32 {
    fn from(value: LegacyTelemetryType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for LegacyTelemetryType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyTelemetryType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

// Domain: text

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextPacketType {
    #[default]
    Raw,
    Chat,
    Translate,
    Popup,
    JukeboxPopup,
    Tip,
    SystemMessage,
    Whisper,
    Announcement,
    TextObjectWhisper,
    TextObject,
    TextObjectAnnouncement,
    Unknown(u8),
}

impl From<u8> for TextPacketType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Raw,
            1 => Self::Chat,
            2 => Self::Translate,
            3 => Self::Popup,
            4 => Self::JukeboxPopup,
            5 => Self::Tip,
            6 => Self::SystemMessage,
            7 => Self::Whisper,
            8 => Self::Announcement,
            9 => Self::TextObjectWhisper,
            10 => Self::TextObject,
            11 => Self::TextObjectAnnouncement,
            value => Self::Unknown(value),
        }
    }
}

impl TextPacketType {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Raw => 0,
            Self::Chat => 1,
            Self::Translate => 2,
            Self::Popup => 3,
            Self::JukeboxPopup => 4,
            Self::Tip => 5,
            Self::SystemMessage => 6,
            Self::Whisper => 7,
            Self::Announcement => 8,
            Self::TextObjectWhisper => 9,
            Self::TextObject => 10,
            Self::TextObjectAnnouncement => 11,
            Self::Unknown(value) => value,
        }
    }
}

impl From<TextPacketType> for u8 {
    fn from(value: TextPacketType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TextPacketType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TextPacketType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextProcessingEventOrigin {
    #[default]
    Unknown,
    ServerChatPublic,
    ServerChatWhisper,
    SignText,
    AnvilText,
    BookAndQuillText,
    CommandBlockText,
    BlockActorDataText,
    JoinEventText,
    LeaveEventText,
    SlashCommandChat,
    CartographyText,
    KickCommand,
    TitleCommand,
    SummonCommand,
    ServerForm,
    DataDrivenUi,
    Unknown2(i32),
}

impl From<i32> for TextProcessingEventOrigin {
    fn from(value: i32) -> Self {
        match value {
            -1 => Self::Unknown,
            0 => Self::ServerChatPublic,
            1 => Self::ServerChatWhisper,
            2 => Self::SignText,
            3 => Self::AnvilText,
            4 => Self::BookAndQuillText,
            5 => Self::CommandBlockText,
            6 => Self::BlockActorDataText,
            7 => Self::JoinEventText,
            8 => Self::LeaveEventText,
            9 => Self::SlashCommandChat,
            10 => Self::CartographyText,
            11 => Self::KickCommand,
            12 => Self::TitleCommand,
            13 => Self::SummonCommand,
            14 => Self::ServerForm,
            15 => Self::DataDrivenUi,
            value => Self::Unknown2(value),
        }
    }
}

impl TextProcessingEventOrigin {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::ServerChatPublic => 0,
            Self::ServerChatWhisper => 1,
            Self::SignText => 2,
            Self::AnvilText => 3,
            Self::BookAndQuillText => 4,
            Self::CommandBlockText => 5,
            Self::BlockActorDataText => 6,
            Self::JoinEventText => 7,
            Self::LeaveEventText => 8,
            Self::SlashCommandChat => 9,
            Self::CartographyText => 10,
            Self::KickCommand => 11,
            Self::TitleCommand => 12,
            Self::SummonCommand => 13,
            Self::ServerForm => 14,
            Self::DataDrivenUi => 15,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<TextProcessingEventOrigin> for i32 {
    fn from(value: TextProcessingEventOrigin) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for TextProcessingEventOrigin {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TextProcessingEventOrigin {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: types

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EditorLevelMigrationVersion {
    #[default]
    Legacy,
    EditorAchievements,
    Unknown(i8),
}

impl From<i8> for EditorLevelMigrationVersion {
    fn from(value: i8) -> Self {
        match value {
            0 => Self::Legacy,
            1 => Self::EditorAchievements,
            value => Self::Unknown(value),
        }
    }
}

impl EditorLevelMigrationVersion {
    pub fn to_raw(self) -> i8 {
        match self {
            Self::Legacy => 0,
            Self::EditorAchievements => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EditorLevelMigrationVersion> for i8 {
    fn from(value: EditorLevelMigrationVersion) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EditorLevelMigrationVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EditorLevelMigrationVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::I8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EditorWorldType {
    #[default]
    NonEditor,
    EditorProject,
    EditorTestLevel,
    EditorRealmsUpload,
    Unknown(i32),
}

impl From<i32> for EditorWorldType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::NonEditor,
            1 => Self::EditorProject,
            2 => Self::EditorTestLevel,
            3 => Self::EditorRealmsUpload,
            value => Self::Unknown(value),
        }
    }
}

impl EditorWorldType {
    pub fn to_raw(self) -> i32 {
        match self {
            Self::NonEditor => 0,
            Self::EditorProject => 1,
            Self::EditorTestLevel => 2,
            Self::EditorRealmsUpload => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<EditorWorldType> for i32 {
    fn from(value: EditorWorldType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for EditorWorldType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EditorWorldType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::ZigZag32 as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinAnimatedTextureType {
    #[default]
    None,
    Face,
    Body32x32,
    Body128x128,
    Unknown(u32),
}

impl From<u32> for SkinAnimatedTextureType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Face,
            2 => Self::Body32x32,
            3 => Self::Body128x128,
            value => Self::Unknown(value),
        }
    }
}

impl SkinAnimatedTextureType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Face => 1,
            Self::Body32x32 => 2,
            Self::Body128x128 => 3,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SkinAnimatedTextureType> for u32 {
    fn from(value: SkinAnimatedTextureType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SkinAnimatedTextureType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SkinAnimatedTextureType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinAnimationExpression {
    #[default]
    Linear,
    Blinking,
    Unknown(u32),
}

impl From<u32> for SkinAnimationExpression {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Linear,
            1 => Self::Blinking,
            value => Self::Unknown(value),
        }
    }
}

impl SkinAnimationExpression {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Linear => 0,
            Self::Blinking => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SkinAnimationExpression> for u32 {
    fn from(value: SkinAnimationExpression) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SkinAnimationExpression {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SkinAnimationExpression {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(
            <wire::VarUInt as wire::Decode>::decode(reader)?.0,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinArmSize {
    #[default]
    Slim,
    Wide,
    Unknown(u8),
}

impl From<u8> for SkinArmSize {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Slim,
            1 => Self::Wide,
            value => Self::Unknown(value),
        }
    }
}

impl SkinArmSize {
    pub fn to_raw(self) -> u8 {
        match self {
            Self::Slim => 0,
            Self::Wide => 1,
            Self::Unknown(value) => value,
        }
    }
}

impl From<SkinArmSize> for u8 {
    fn from(value: SkinArmSize) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SkinArmSize {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U8(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SkinArmSize {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U8 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SkinPieceType {
    #[default]
    Unknown,
    Skeleton,
    Body,
    Skin,
    Bottom,
    Feet,
    Dress,
    Top,
    HighPants,
    Hands,
    Outerwear,
    FacialHair,
    Mouth,
    Eyes,
    Hair,
    Hood,
    Back,
    FaceAccessory,
    Head,
    Legs,
    LeftLeg,
    RightLeg,
    Arms,
    LeftArm,
    RightArm,
    Capes,
    ClassicSkin,
    Emote,
    CoCo,
    Unsupported,
    Unknown2(u32),
}

impl From<u32> for SkinPieceType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::Skeleton,
            2 => Self::Body,
            3 => Self::Skin,
            4 => Self::Bottom,
            5 => Self::Feet,
            6 => Self::Dress,
            7 => Self::Top,
            8 => Self::HighPants,
            9 => Self::Hands,
            10 => Self::Outerwear,
            11 => Self::FacialHair,
            12 => Self::Mouth,
            13 => Self::Eyes,
            14 => Self::Hair,
            15 => Self::Hood,
            16 => Self::Back,
            17 => Self::FaceAccessory,
            18 => Self::Head,
            19 => Self::Legs,
            20 => Self::LeftLeg,
            21 => Self::RightLeg,
            22 => Self::Arms,
            23 => Self::LeftArm,
            24 => Self::RightArm,
            25 => Self::Capes,
            26 => Self::ClassicSkin,
            27 => Self::Emote,
            28 => Self::CoCo,
            29 => Self::Unsupported,
            value => Self::Unknown2(value),
        }
    }
}

impl SkinPieceType {
    pub fn to_raw(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Skeleton => 1,
            Self::Body => 2,
            Self::Skin => 3,
            Self::Bottom => 4,
            Self::Feet => 5,
            Self::Dress => 6,
            Self::Top => 7,
            Self::HighPants => 8,
            Self::Hands => 9,
            Self::Outerwear => 10,
            Self::FacialHair => 11,
            Self::Mouth => 12,
            Self::Eyes => 13,
            Self::Hair => 14,
            Self::Hood => 15,
            Self::Back => 16,
            Self::FaceAccessory => 17,
            Self::Head => 18,
            Self::Legs => 19,
            Self::LeftLeg => 20,
            Self::RightLeg => 21,
            Self::Arms => 22,
            Self::LeftArm => 23,
            Self::RightArm => 24,
            Self::Capes => 25,
            Self::ClassicSkin => 26,
            Self::Emote => 27,
            Self::CoCo => 28,
            Self::Unsupported => 29,
            Self::Unknown2(value) => value,
        }
    }
}

impl From<SkinPieceType> for u32 {
    fn from(value: SkinPieceType) -> Self {
        value.to_raw()
    }
}

impl wire::Encode for SkinPieceType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U32LE(self.to_raw()).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SkinPieceType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self::from(<wire::U32LE as wire::Decode>::decode(reader)?.0))
    }
}
