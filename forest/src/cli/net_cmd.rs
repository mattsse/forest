// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use super::stringify_rpc_err;
use cid::Cid;
use rpc_client::{block, genesis, head, messages, new_client, read_obj};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum NetCommands {
    #[structopt(about = "Print Peers")]
    Peers,

    #[structopt(about = "Connect to a Peer")]
    Connect {
        #[structopt(help = "Peer address given as a string")]
        peer_addr: String,
    },

    #[structopt(about = "List listen Address")]
    Listen,

    #[structopt(about = "Get node address")]
    Id,

    #[structopt(about = "Find the address of a given Peer Id")]
    FindPeer {
        #[structopt(help = "Peer id given as a string")]
        peer_id: String,
    },

    #[structopt(about = "Print peers' pubsub scores")]
    Scores,
}

impl NetCommands {
    pub async fn run(&self) {
        println!("Running Net Commands");

        match self {
            Self::Peers => {}
            Self::Connect { peer_addr } => {}
            Self::Listen => {}
            Self::Id => {}
            Self::FindPeer { peer_id } => {}
            Self::Scores => {}
        }
    }
}
