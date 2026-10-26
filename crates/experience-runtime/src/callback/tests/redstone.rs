//! Server WIT 0.12's redstone in callbacks: the power a redstone block receives and emits.

use super::*;

/// [`Fixture::new`] with the Experience's redstone block at [`ANCHOR`], receiving `power`.
fn emitter(power: u8) -> Fixture {
    let mut fixture = Fixture::new().cell(ANCHOR, EMITTER, true, None);
    fixture.cells[0].power = power;
    fixture
}

/// A redstone block reads the power its snapshot cell receives; another's block is not owned and
/// one of the Experience's blocks that is not a redstone block takes no power.
#[test]
fn redstone_power_reads_own_redstone_blocks() {
    let mut res = emitter(9)
        .cell(UP, COUNTER, true, None)
        .cell(EAST, "minecraft:stone", false, None)
        .res();
    assert_eq!(res.redstone_power(ANCHOR).unwrap(), Ok(9));
    assert_eq!(
        res.redstone_power(UP).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(res.redstone_power(EAST).unwrap(), Err(WorldError::NotOwned));
}

/// Setting the power stages one op per position, which a later write replaces; a power above 15
/// or a block that is not a redstone block is refused and stages nothing.
#[test]
fn set_redstone_power_stages_one_op_per_block() {
    let mut res = emitter(0).cell(UP, COUNTER, true, None).res();
    assert_eq!(res.set_redstone_power(ANCHOR, 15).unwrap(), Ok(()));
    assert_eq!(res.set_redstone_power(ANCHOR, 7).unwrap(), Ok(()));
    assert_eq!(
        res.set_redstone_power(ANCHOR, 16).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(
        res.set_redstone_power(UP, 1).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(
        res.set_redstone_power(EAST, 1).unwrap(),
        Err(WorldError::NotOwned)
    );
    assert_eq!(
        res.ops,
        vec![Op::SetRedstonePower {
            pos: ANCHOR,
            power: 7
        }]
    );
}

/// Replacing a redstone block drops the power staged for it before the replacement.
#[test]
fn replacement_drops_power_staged_before_it() {
    let mut res = emitter(0).res();
    assert_eq!(res.set_redstone_power(ANCHOR, 15).unwrap(), Ok(()));
    assert_eq!(res.set_block(ANCHOR, EMITTER.to_owned()).unwrap(), Ok(()));
    assert_eq!(
        res.ops,
        vec![Op::SetBlock {
            pos: ANCHOR,
            id: EMITTER.to_owned()
        }]
    );
}

/// A snapshot whose cell receives more than 15, or power at a block that is not its own
/// redstone block, is malformed.
#[test]
fn malformed_power_is_refused() {
    assert!(emitter(15).prepared_with_emitter().is_ok());
    assert!(emitter(16).prepared_with_emitter().is_err());
    let mut counter = Fixture::new();
    counter.cells[0].power = 1;
    assert!(counter.prepared_with_emitter().is_err());
}

impl Fixture {
    /// [`Fixture::prepared`] for an Experience whose [`EMITTER`] is a redstone block.
    fn prepared_with_emitter(self) -> Result<CallbackRes, String> {
        let request = Request::Callback {
            seq: 1,
            info: Info {
                context: None,
                simulation: None,
                world_id: "world".to_owned(),
                dimension_id: "overworld".to_owned(),
                tick: 1,
                event_sequence: 1,
            },
            actor: self.actor.map(str::to_owned),
            world_min_y: self.min_y,
            world_max_y: self.max_y,
            data_budget: self.budget,
            snapshot: self.cells,
            network: self.network,
            inventory: self.inventory,
            containers: self.containers,
            call: Call::Interact {
                player: ACTOR.to_owned(),
                pos: ANCHOR,
                face: Face::Up,
                hit: None,
            },
        };
        let own = Catalog {
            blocks: [
                (COUNTER.to_owned(), Vec::new()),
                (EMITTER.to_owned(), Vec::new()),
            ]
            .into(),
            redstone_blocks: [EMITTER.to_owned()].into(),
            ..Catalog::default()
        };
        prepare(&Arc::new(own), &request).map(|(res, _)| res)
    }
}
