use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

mod bls;
pub use bls::{SerializableG1Affine, SerializableScalar};

pub mod rpc;
pub mod dkg;
pub mod p2p;


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PeerInfo {
    pub addresses: Vec<Multiaddr>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub enum NetworkMessage {
    Broadcast {
        topic: String,
        data: Vec<u8>,
    },
    DirectMessage {
        peer_id: String,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyShare {
    pub index: usize,
    pub value: SerializableScalar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipsubMessage {
    DKG(crate::dkg::DKGMessage),
    Signing(crate::signing::SigningMessage),
    Command(crate::commands::Command),
    Consensus(crate::consensus::ConsensusMessage),
}