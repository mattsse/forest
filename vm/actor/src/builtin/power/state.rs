// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use super::CONSENSUS_MINER_MIN_POWER;
use crate::{smooth::FilterEstimate, BytesKey, Multimap, HAMT_BIT_WIDTH};
use address::Address;
use cid::Cid;
use clock::{ChainEpoch, EPOCH_UNDEFINED};
use encoding::{tuple::*, Cbor};
use fil_types::StoragePower;
use integer_encoding::VarInt;
use ipld_blockstore::BlockStore;
use ipld_hamt::Hamt;
use num_bigint::{bigint_ser, BigInt};
use vm::{Serialized, TokenAmount};

lazy_static! {
    /// genesis power in bytes = 750,000 GiB
    static ref INITIAL_QA_POWER_ESTIMATE_POSITION: BigInt = BigInt::from(750_000) * (1 << 30);
    /// max chain throughput in bytes per epoch = 120 ProveCommits / epoch = 3,840 GiB
    static ref INITIAL_QA_POWER_ESTIMATE_VELOCITY: BigInt = BigInt::from(3_840) * (1 << 30);
}

/// Storage power actor state
#[derive(Default, Serialize_tuple, Deserialize_tuple)]
pub struct State {
    #[serde(with = "bigint_ser")]
    pub total_raw_byte_power: StoragePower,
    #[serde(with = "bigint_ser")]
    pub total_bytes_committed: StoragePower,
    #[serde(with = "bigint_ser")]
    pub total_quality_adj_power: StoragePower,
    #[serde(with = "bigint_ser")]
    pub total_qa_bytes_committed: StoragePower,
    #[serde(with = "bigint_ser")]
    pub total_pledge_collateral: TokenAmount,

    #[serde(with = "bigint_ser")]
    pub this_epoch_raw_byte_power: StoragePower,
    #[serde(with = "bigint_ser")]
    pub this_epoch_quality_adj_power: StoragePower,
    #[serde(with = "bigint_ser")]
    pub this_epoch_pledge_collateral: TokenAmount,
    pub this_epoch_qa_power_smoothed: FilterEstimate,

    pub miner_count: i64,
    /// Number of miners having proven the minimum consensus power.
    pub miner_above_min_power_count: i64,

    /// A queue of events to be triggered by cron, indexed by epoch.
    pub cron_event_queue: Cid, // Multimap, (HAMT[ChainEpoch]AMT[CronEvent]

    /// First epoch in which a cron task may be stored. Cron will iterate every epoch between this
    /// and the current epoch inclusively to find tasks to execute.
    pub first_cron_epoch: ChainEpoch,

    /// Last epoch power cron tick has been processed.
    pub last_processed_cron_epoch: ChainEpoch,

    /// Claimed power for each miner.
    pub claims: Cid, // Map, HAMT[address]Claim

    pub proof_validation_batch: Option<Cid>,
}

impl State {
    pub fn new(empty_map_cid: Cid, empty_mmap_cid: Cid) -> State {
        State {
            cron_event_queue: empty_mmap_cid,
            claims: empty_map_cid,
            last_processed_cron_epoch: EPOCH_UNDEFINED,
            this_epoch_qa_power_smoothed: FilterEstimate {
                position: INITIAL_QA_POWER_ESTIMATE_POSITION.clone(),
                velocity: INITIAL_QA_POWER_ESTIMATE_VELOCITY.clone(),
            },
            ..Default::default()
        }
    }

    // TODO minerNominalPowerMeetsConsensusMinimum

    pub fn add_to_claim<BS: BlockStore>(
        &mut self,
        store: &BS,
        miner: &Address,
        power: &StoragePower,
        qa_power: &StoragePower,
    ) -> Result<(), String> {
        let mut claim = self
            .get_claim(store, miner)?
            .ok_or(format!("no claim for actor {}", miner))?;

        let old_nominal_power = claim.quality_adj_power.clone();

        // update power
        claim.raw_byte_power += power;
        claim.quality_adj_power += qa_power;

        let new_nominal_power = &claim.quality_adj_power;

        let min_power_ref: &StoragePower = &*CONSENSUS_MINER_MIN_POWER;
        let prev_below: bool = &old_nominal_power < min_power_ref;
        let still_below: bool = new_nominal_power < min_power_ref;

        if prev_below && !still_below {
            // Just passed min miner size
            self.miner_above_min_power_count += 1;
            self.total_quality_adj_power += new_nominal_power;
            self.total_raw_byte_power += &claim.raw_byte_power;
        } else if !prev_below && still_below {
            // just went below min miner size
            self.miner_above_min_power_count -= 1;
            self.total_quality_adj_power = self
                .total_quality_adj_power
                .checked_sub(&old_nominal_power)
                .ok_or("Negative nominal power")?;
            self.total_raw_byte_power = self
                .total_raw_byte_power
                .checked_sub(&claim.raw_byte_power)
                .ok_or("Negative raw byte power")?;
        } else if !prev_below && !still_below {
            // Was above the threshold, still above
            self.total_quality_adj_power += qa_power;
            self.total_raw_byte_power += power;
        }

        if self.miner_above_min_power_count < 0 {
            return Err(format!(
                "negative number of miners: {}",
                self.miner_above_min_power_count
            ));
        }

        self.set_claim(store, miner, claim)
    }

    pub(super) fn add_pledge_total(&mut self, amount: TokenAmount) {
        self.total_pledge_collateral += amount;
    }

    pub(super) fn append_cron_event<BS: BlockStore>(
        &mut self,
        s: &BS,
        epoch: ChainEpoch,
        event: CronEvent,
    ) -> Result<(), String> {
        let mut mmap = Multimap::from_root(s, &self.cron_event_queue)?;
        mmap.add(epoch_key(epoch), event)?;
        self.cron_event_queue = mmap.root()?;
        Ok(())
    }

    pub(super) fn load_cron_events<BS: BlockStore>(
        &mut self,
        s: &BS,
        epoch: ChainEpoch,
    ) -> Result<Vec<CronEvent>, String> {
        let mut events = Vec::new();

        let mmap = Multimap::from_root(s, &self.cron_event_queue)?;
        mmap.for_each(&epoch_key(epoch), |_, v: &CronEvent| {
            match self.get_claim(s, &v.miner_addr) {
                Ok(Some(_)) => events.push(v.clone()),
                Err(e) => {
                    return Err(format!(
                        "failed to find claimed power for {} for cron event: {}",
                        v.miner_addr, e
                    ))
                }
                _ => (), // ignore events for defunct miners.
            }
            Ok(())
        })?;

        Ok(events)
    }

    pub(super) fn clear_cron_events<BS: BlockStore>(
        &mut self,
        s: &BS,
        epoch: ChainEpoch,
    ) -> Result<(), String> {
        let mut mmap = Multimap::from_root(s, &self.cron_event_queue)?;
        mmap.remove_all(&epoch_key(epoch))?;
        self.cron_event_queue = mmap.root()?;
        Ok(())
    }

    /// Gets claim from claims map by address
    pub fn get_claim<BS: BlockStore>(
        &self,
        store: &BS,
        a: &Address,
    ) -> Result<Option<Claim>, String> {
        let map: Hamt<BytesKey, _> =
            Hamt::load_with_bit_width(&self.claims, store, HAMT_BIT_WIDTH)?;

        Ok(map.get(&a.to_bytes())?)
    }

    pub(super) fn set_claim<BS: BlockStore>(
        &mut self,
        store: &BS,
        addr: &Address,
        claim: Claim,
    ) -> Result<(), String> {
        let mut map: Hamt<BytesKey, _> =
            Hamt::load_with_bit_width(&self.claims, store, HAMT_BIT_WIDTH)?;

        map.set(addr.to_bytes().into(), claim)?;
        self.claims = map.flush()?;
        Ok(())
    }

    pub(super) fn delete_claim<BS: BlockStore>(
        &mut self,
        store: &BS,
        addr: &Address,
    ) -> Result<(), String> {
        let mut map: Hamt<BytesKey, _> =
            Hamt::load_with_bit_width(&self.claims, store, HAMT_BIT_WIDTH)?;

        map.delete(&addr.to_bytes())?;
        self.claims = map.flush()?;
        Ok(())
    }
}

fn epoch_key(e: ChainEpoch) -> BytesKey {
    let bz = e.encode_var_vec();
    bz.into()
}

impl Cbor for State {}

#[derive(Default, Debug, Serialize_tuple, Deserialize_tuple)]
pub struct Claim {
    /// Sum of raw byte power for a miner's sectors.
    #[serde(with = "bigint_ser")]
    pub raw_byte_power: StoragePower,
    /// Sum of quality adjusted power for a miner's sectors.
    #[serde(with = "bigint_ser")]
    pub quality_adj_power: StoragePower,
}

#[derive(Clone, Debug, Serialize_tuple, Deserialize_tuple)]
pub struct CronEvent {
    pub miner_addr: Address,
    pub callback_payload: Serialized,
}

impl Cbor for CronEvent {}

#[cfg(test)]
mod test {
    use super::*;
    use clock::ChainEpoch;

    #[test]
    fn epoch_key_test() {
        let e1: ChainEpoch = 101;
        let e2: ChainEpoch = 102;
        let e3: ChainEpoch = 103;
        let e4: ChainEpoch = -1;

        let b1: BytesKey = [0xca, 0x1].to_vec().into();
        let b2: BytesKey = [0xcc, 0x1].to_vec().into();
        let b3: BytesKey = [0xce, 0x1].to_vec().into();
        let b4: BytesKey = [0x1].to_vec().into();

        assert_eq!(b1, epoch_key(e1));
        assert_eq!(b2, epoch_key(e2));
        assert_eq!(b3, epoch_key(e3));
        assert_eq!(b4, epoch_key(e4));
    }
}
