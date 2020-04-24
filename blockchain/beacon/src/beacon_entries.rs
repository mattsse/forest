// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use encoding::{
    de::{self, Deserializer},
    ser::{self, Serializer},
    Cbor, Error as EncodingError,
};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BeaconEntry {
    round: u64,
    data: Vec<u8>,
    prev_round: u64,
}

impl BeaconEntry {
    pub fn new(round: u64, data: Vec<u8>, prev_round: u64) -> Self {
        Self {
            round,
            data,
            prev_round,
        }
    }
    pub fn round(&self) -> u64 {
        self.round
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn prev_round(&self) -> u64 {
        self.prev_round
    }
}

impl ser::Serialize for BeaconEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (&self.round, &self.data, &self.prev_round).serialize(serializer)
    }
}

impl<'de> de::Deserialize<'de> for BeaconEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let (round, data, prev_round) = Deserialize::deserialize(deserializer)?;

        Ok(Self {
            round,
            data,
            prev_round,
        })
    }
}