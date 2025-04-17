use bls12_381::{G1Affine, G1Projective, Scalar};
use ff::Field;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::types::SerializableG1Affine;

#[derive(Debug, Serialize, Deserialize)]
pub struct Commitment {
    pub D: SerializableG1Affine,
    pub E: SerializableG1Affine,
}

#[derive(Debug)]
pub struct FrostSigner {
    pub id: String,
    secret_share: Scalar,
    public_key: G1Projective,
    nonces: Vec<(Scalar, Scalar)>,
    commitments: HashMap<String, Vec<Commitment>>,
}

impl FrostSigner {
    pub fn new(id: String, secret_share: Scalar, public_key: G1Projective) -> Self {
        Self {
            id,
            secret_share,
            public_key,
            nonces: Vec::new(),
            commitments: HashMap::new(),
        }
    }

    pub fn generate_nonces(&mut self, count: usize) {
        let mut rng = OsRng;
        for _ in 0..count {
            let d = Scalar::random(&mut rng);
            let e = Scalar::random(&mut rng);
            self.nonces.push((d, e));
        }
    }

    pub fn get_commitments(&self) -> Vec<Commitment> {
        self.nonces
            .iter()
            .map(|(d, e)| Commitment {
                D: SerializableG1Affine(G1Affine::from(G1Projective::generator() * d)),
                E: SerializableG1Affine(G1Affine::from(G1Projective::generator() * e)),
            })
            .collect()
    }

    pub fn sign(&mut self, message: &[u8], participants: &[String]) -> Option<Scalar> {
        if self.nonces.is_empty() {
            return None;
        }

        let (d, e) = self.nonces.remove(0);
        let commitment = self.get_commitments().get(0);
        
        // Implement actual FROST signing logic here
        Some(self.secret_share)
    }
}