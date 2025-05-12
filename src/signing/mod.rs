use std::collections::HashMap;
use anyhow::Result;
use bls12_381::{G1Affine, G1Projective, Scalar};
use ff::Field;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};

use crate::network::NetworkLayer;
use crate::types::{KeyShare, ChannelMessage, SerializableG1Affine, SerializableScalar};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostCommitment {
    pub D: SerializableG1Affine,
    pub E: SerializableG1Affine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SigningMessage {
    CommitmentShare {
        from: String,
        commitments: Vec<FrostCommitment>,
    },
    SignatureShare {
        from: String,
        share: SerializableScalar,
    },
}

pub struct SigningNode {
    id: String,
    threshold: usize,
    key_share: KeyShare,
    group_key: G1Projective,
    nonces: Vec<(Scalar, Scalar)>,
    commitments: RwLock<HashMap<String, Vec<FrostCommitment>>>,
    signature_shares: RwLock<HashMap<String, Scalar>>,
    message_tx: mpsc::Sender<ChannelMessage>,
}

impl SigningNode {
    pub fn new(
        id: String,
        threshold: usize,
        key_share: KeyShare,
        group_key: G1Projective,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            id,
            threshold,
            key_share,
            group_key,
            nonces: Vec::new(),
            commitments: RwLock::new(HashMap::new()),
            signature_shares: RwLock::new(HashMap::new()),
            message_tx
        }
    }

    pub async fn broadcast(&self, topic: &str, data: &[u8]) -> Result<()> {
        self.message_tx.send(ChannelMessage::Broadcast {
            topic: topic.to_string(),
            data: data.to_vec(),
        }).await?;
        Ok(())
    }

    pub async fn start_signing(&mut self, message: &[u8]) -> Result<()> {
        self.generate_nonces(1);
        let commitments = self.get_commitments();
        
        let msg = serde_json::to_vec(&SigningMessage::CommitmentShare {
            from: self.id.clone(),
            commitments,
        })?;
        
        self.broadcast("signing", &msg).await?;
        Ok(())
    }

    fn generate_nonces(&mut self, count: usize) {
        let mut rng = OsRng;
        for _ in 0..count {
            let d = Scalar::random(&mut rng);
            let e = Scalar::random(&mut rng);
            self.nonces.push((d, e));
        }
    }

    fn get_commitments(&self) -> Vec<FrostCommitment> {
        self.nonces
            .iter()
            .map(|(d, e)| FrostCommitment {
                D: SerializableG1Affine(G1Affine::from(G1Projective::generator() * d)),
                E: SerializableG1Affine(G1Affine::from(G1Projective::generator() * e)),
            })
            .collect()
    }

    pub async fn handle_message(&mut self, msg: SigningMessage) -> Result<()> {
        match msg {
            SigningMessage::CommitmentShare { from, commitments } => {
                self.handle_commitment_share(from, commitments).await?;
            }
            SigningMessage::SignatureShare { from, share } => {
                self.handle_signature_share(from, share.0).await?;
            }
        }
        Ok(())
    }

    async fn handle_commitment_share(
        &mut self,
        from: String,
        commitments: Vec<FrostCommitment>,
    ) -> Result<()> {
        self.commitments.write().await.insert(from, commitments);
        self.check_commitments().await?;
        Ok(())
    }

    async fn check_commitments(&self) -> Result<()> {
        let commitments = self.commitments.read().await;
        if commitments.len() >= self.threshold {
            // Generate and broadcast signature share
            let signature_share = self.generate_signature_share().await?;
            let msg = serde_json::to_vec(&SigningMessage::SignatureShare {
                from: self.id.clone(),
                share: SerializableScalar(signature_share),
            })?;
            self.broadcast("signing", &msg).await?;
        }
        Ok(())
    }

    async fn generate_signature_share(&self) -> Result<Scalar> {
        // Implement FROST signature share generation
        Ok(Scalar::zero()) // Placeholder
    }

    async fn handle_signature_share(
        &mut self,
        from: String,
        share: Scalar,
    ) -> Result<()> {
        self.signature_shares.write().await.insert(from, share);
        self.check_signature_completion().await?;
        Ok(())
    }

    async fn check_signature_completion(&self) -> Result<()> {
        let shares = self.signature_shares.read().await;
        if shares.len() >= self.threshold {
            let signature = self.aggregate_signature_shares().await?;
            // Handle completed signature
        }
        Ok(())
    }

    async fn aggregate_signature_shares(&self) -> Result<Scalar> {
        let shares = self.signature_shares.read().await;
        let mut signature = Scalar::zero();
        
        for share in shares.values() {
            signature += *share;
        }
        
        Ok(signature)
    }
}