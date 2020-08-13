
use std::sync::Mutex;
use super::snapshot::PeerScoreSnapshot;
use std::collections::HashMap;
use libp2p::core::PeerId;

#[derive(Default)]
pub struct ScoreKeeper {
	pub scores : Mutex<HashMap<PeerId, PeerScoreSnapshot>>,
}

impl ScoreKeeper{
    pub fn update(&mut self, scores : HashMap<PeerId, PeerScoreSnapshot> ) {
        *self.scores.lock().unwrap() = scores;
    }
}