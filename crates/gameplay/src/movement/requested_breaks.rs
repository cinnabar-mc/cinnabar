//! Explicit destruction overrides carried by the same authority and retry path as movement.

use super::MovementTicker;
use crate::{mining::QueuedMiningInteraction, survival_mining::CreativeDestroyBlock};
use protocol::{BlockAction, BlockActionKind, BlockUseRequest};

impl MovementTicker {
    /// Attaches a validated explicit batch atomically to the newest unsent movement sample.
    /// Requests bypass mining progress, while replay, retries and authority revocation remain
    /// those of the carrier. Returns false without changing the carrier when it cannot accept it.
    pub fn attach_requested_breaks(
        &mut self,
        requests: &[BlockUseRequest],
        server_authoritative: bool,
        creative_actor: Option<u64>,
    ) -> bool {
        if !self.accepts_block_interactions() || self.settling_position_authority() {
            return false;
        }
        if requests
            .iter()
            .any(|request| protocol::destroy_block_packet(request.clone()).is_err())
        {
            return false;
        }
        let Some(sample) = self.outbox.back_mut() else {
            return false;
        };
        if sample
            .mining
            .as_ref()
            .is_some_and(|mining| mining.requested_breaks)
        {
            return false;
        }
        let mining = sample
            .mining
            .get_or_insert_with(|| QueuedMiningInteraction {
                creative_destroy: None,
                requested_breaks: false,
                requested_destroys: Vec::new(),
                requested_creative_destroys: Vec::new(),
                interactions: Default::default(),
                mining_request: None,
            });
        mining.requested_breaks = true;
        for request in requests {
            if server_authoritative {
                for kind in [
                    BlockActionKind::StartDestroy,
                    BlockActionKind::PredictDestroy,
                    BlockActionKind::StopDestroy,
                ] {
                    mining.interactions.block_actions.push(BlockAction {
                        kind,
                        position: request.block_position,
                        face: request.face,
                    });
                }
            } else {
                let mut request = request.clone();
                request.player_position = sample.snapshot.position;
                mining.requested_destroys.push(request);
            }
            if let Some(local_runtime_id) = creative_actor {
                mining
                    .requested_creative_destroys
                    .push(CreativeDestroyBlock {
                        local_runtime_id,
                        position: request.block_position,
                        face: request.face,
                    });
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::survival_mining::ticker_with_ticks;

    /// One validated mainhand destruction request, with its current item.
    fn request(x: i32) -> BlockUseRequest {
        BlockUseRequest {
            hand: protocol::ItemUseHand::Mainhand,
            block_position: [x, 2, 0],
            face: 0,
            selected_slot: 0,
            selected_item: protocol::VerifiedNetworkItemStack::try_new(
                protocol::NetworkItemStack::empty(),
                protocol::NetworkItemStack::empty().nbt_digest,
            )
            .unwrap(),
            player_position: [0.; 3],
            relative_hit: [0.5; 3],
            block_runtime_id: 1,
        }
    }

    #[test]
    fn entire_server_batch_shares_one_tick_without_replacing_existing_mining() {
        let mut ticker = ticker_with_ticks(1);
        let ordinary = crate::survival_mining::SurvivalTickPayload::default();
        assert!(ticker.attach_survival_mining(101, ordinary));
        assert!(ticker.attach_requested_breaks(&[request(1), request(2)], true, None));
        let mining = ticker.outbox.back().unwrap().mining.as_ref().unwrap();
        let actions: Vec<_> = mining.interactions.block_actions.iter().collect();
        assert_eq!(actions.len(), 6);
        assert_eq!(actions[1].kind, BlockActionKind::PredictDestroy);
        assert_eq!(actions[1].position, [1, 2, 0]);
        assert_eq!(actions[4].position, [2, 2, 0]);
        assert!(mining.requested_destroys.is_empty());
        assert!(!ticker.attach_requested_breaks(&[request(3)], true, None));
    }

    #[test]
    fn client_batch_and_creative_actions_keep_each_block_and_replay_position() {
        let mut ticker = ticker_with_ticks(1);
        assert!(ticker.attach_requested_breaks(&[request(1), request(2)], false, Some(42)));
        let mining = ticker.outbox.back_mut().unwrap().mining.as_mut().unwrap();
        assert_eq!(mining.requested_destroys.len(), 2);
        assert_eq!(mining.requested_creative_destroys.len(), 2);
        assert!(mining.interactions.block_actions.is_empty());
        mining.replay_at([3., 4., 5.]);
        assert!(
            mining
                .requested_destroys
                .iter()
                .all(|r| r.player_position == [3., 4., 5.])
        );
    }

    #[test]
    fn missing_carrier_and_invalid_batch_leave_no_partial_actions() {
        assert!(!ticker_with_ticks(0).attach_requested_breaks(&[request(1)], true, None));
        let mut ticker = ticker_with_ticks(1);
        let mut invalid = request(2);
        invalid.face = 255;
        assert!(!ticker.attach_requested_breaks(&[request(1), invalid], true, None));
        assert!(ticker.outbox.back().unwrap().mining.is_none());
    }
}
