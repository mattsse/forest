use clock::ChainEpoch;
use std::collections::HashMap;


#[derive(Default)]
pub struct PeerScoreSnapshot {
	pub score              : f64,
	pub topics             : HashMap<String, TopicScoreSnapshot>,
	pub app_specific_score   : f64,
	pub colocation_factor : f64,
	pub behaviour_penalty   : f64,
}

#[derive(Default)]
pub struct TopicScoreSnapshot {
	pub time_in_mesh : ChainEpoch,
	pub first_deliveries   : f64,
	pub mesh_deliveries    : f64,
	pub invalid_deliveries : f64,
}