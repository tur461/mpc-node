use bls12_381::{G1Affine, Scalar};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use super::{p2p::SerializablePeerId, SerializableG1Affine, SerializableScalar};




pub const MSG_TOPIC_DKG: &str = "topic-dkg";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ADKGMessage {
    
}