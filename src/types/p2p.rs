use libp2p::PeerId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use serde::de::{self, Visitor};

#[derive(Debug, Clone)]
pub struct SerializablePeerId(pub PeerId);

impl Serialize for SerializablePeerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = self.0.to_bytes();
        serializer.serialize_bytes(&bytes)
    }
}

struct PeerIdVisitor;

impl<'de> Visitor<'de> for PeerIdVisitor {
    type Value = SerializablePeerId;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a peerId in bytes form")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v.len() != 48 {
            return Err(E::custom("invalid length for peerId"));
        }
        
        let mut bytes = [0u8; 48];
        bytes.copy_from_slice(v);
        
        PeerId::from_bytes(&bytes)
            .map(SerializablePeerId)
            .map_err(|_| E::custom("invalid peerId"))
    }
}

impl<'de> Deserialize<'de> for SerializablePeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(PeerIdVisitor)
    }
}
