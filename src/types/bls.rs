use bls12_381::{G1Affine, Scalar, G1Projective};
use group::GroupEncoding;
use serde::{Deserialize, Serialize, Serializer, Deserializer};
use serde::de::{self, Visitor};
use std::fmt;

#[derive(Debug, Clone)]
pub struct SerializableG1Affine(pub G1Affine);

impl Serialize for SerializableG1Affine {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize G1Affine as compressed bytes
        let bytes = self.0.to_compressed();
        serializer.serialize_bytes(&bytes)
    }
}

struct G1AffineVisitor;

impl<'de> Visitor<'de> for G1AffineVisitor {
    type Value = SerializableG1Affine;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a G1Affine point in compressed form")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v.len() != 48 {
            return Err(E::custom("invalid length for G1Affine"));
        }
        
        let mut bytes = [0u8; 48];
        bytes.copy_from_slice(v);
        
        Option::from(G1Affine::from_compressed(&bytes))
            .map(SerializableG1Affine)
            .ok_or_else(|| E::custom("invalid G1Affine point"))
    }
}

impl<'de> Deserialize<'de> for SerializableG1Affine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(G1AffineVisitor)
    }
}

#[derive(Debug, Clone)]
pub struct SerializableScalar(pub Scalar);

impl Serialize for SerializableScalar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = self.0.to_bytes();
        serializer.serialize_bytes(&bytes)
    }
}

struct ScalarVisitor;

impl<'de> Visitor<'de> for ScalarVisitor {
    type Value = SerializableScalar;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a Scalar value in bytes form")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v.len() != 32 {
            return Err(E::custom("invalid length for Scalar"));
        }
        
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(v);
        
        Option::from(Scalar::from_bytes(&bytes))
            .map(SerializableScalar)
            .ok_or_else(|| E::custom("invalid Scalar value"))
    }
}

impl<'de> Deserialize<'de> for SerializableScalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(ScalarVisitor)
    }
}