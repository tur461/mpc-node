use std::{any, usize};
use std::collections::HashMap;
use anyhow::Result;
use bls12_381::{G1Affine, G1Projective, Scalar};
use ethers::core::k256::ecdsa::Error;
use ff::Field;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::sync::{mpsc};

use crate::types::dkg::MSG_TOPIC_DKG;
use crate::types::{KeyShare, ChannelMessage, SerializableG1Affine, SerializableScalar};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DKGMessage {
    ShareDistribution {
        from: String,
        shares: Vec<KeyShare>,
        commitments: Vec<SerializableG1Affine>,
    },
    ShareValidation {
        from: String,
        to: String,
        is_valid: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DKGStatus {
    New,
    Error {kind: String},
    Started,
    Completed,
}

pub struct DKGNode {
    id: String,
    status: DKGStatus,
    threshold: usize,
    total_nodes: usize,
    shares: RwLock<HashMap<String, KeyShare>>,
    commitments: RwLock<HashMap<String, Vec<G1Affine>>>,
    validations: RwLock<HashMap<String, Vec<bool>>>,
    message_tx: mpsc::Sender<ChannelMessage>,
}

impl DKGNode {
    pub fn new(
        id: String,
        threshold: usize,
        total_nodes: usize,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            id,
            status: DKGStatus::New,
            threshold,
            total_nodes,
            shares: RwLock::new(HashMap::new()),
            commitments: RwLock::new(HashMap::new()),
            validations: RwLock::new(HashMap::new()),
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

    pub fn get_status(&self) -> DKGStatus {
        self.status.clone()
    }

    pub async fn start_dkg(&mut self) -> Result<()> {
        let polynomial = self.generate_polynomial()?;
        let shares = self.generate_shares(&polynomial)?;
        let commitments = self.generate_commitments(&polynomial)?;
        self.status = DKGStatus::Started;
        self.broadcast_shares(shares, commitments).await?;
        Ok(())
    }

    fn generate_polynomial(&self) -> Result<Vec<Scalar>> {
        let mut rng = OsRng;
        let mut coefficients = Vec::with_capacity(self.threshold + 1);
        
        for _ in 0..=self.threshold {
            coefficients.push(Scalar::random(&mut rng));
        }
        
        Ok(coefficients)
    }

    fn generate_shares(&self, polynomial: &[Scalar]) -> Result<Vec<KeyShare>> {
        let mut shares = Vec::with_capacity(self.total_nodes);
        
        for i in 1..=self.total_nodes {
            let x = Scalar::from(i as u64);
            let mut y = Scalar::zero();
            
            for (j, coeff) in polynomial.iter().enumerate() {
                y += *coeff * x.pow(&[j as u64, 0, 0, 0]);
            }
            
            shares.push(KeyShare {
                index: i,
                value: SerializableScalar(y),
            });
        }
        
        Ok(shares)
    }

    fn generate_commitments(&self, polynomial: &[Scalar]) -> Result<Vec<SerializableG1Affine>> {
        let commitments: Vec<SerializableG1Affine> = polynomial
            .iter()
            .map(|coeff| SerializableG1Affine(G1Affine::from(G1Projective::generator() * coeff)))
            .collect();
            
        Ok(commitments)
    }

    async fn broadcast_shares(
        &self,
        shares: Vec<KeyShare>,
        commitments: Vec<SerializableG1Affine>,
    ) -> Result<()> {
        let msg = DKGMessage::ShareDistribution {
            from: self.id.clone(),
            shares,
            commitments,
        };
        
        let msg_bytes = serde_json::to_vec(&msg)?;
        self.broadcast(MSG_TOPIC_DKG, &msg_bytes).await?;
        Ok(())
    }

    pub async fn handle_message(&mut self, msg: DKGMessage) -> Result<()> {
        match msg {
            DKGMessage::ShareDistribution { from, shares, commitments } => {
                self.handle_share_distribution(from, shares, commitments).await?;
            }
            DKGMessage::ShareValidation { from, to, is_valid } => {
                self.handle_share_validation(from, to, is_valid).await?;
            }
        }
        Ok(())
    }

    async fn handle_share_distribution(
        &mut self,
        from: String,
        shares: Vec<KeyShare>,
        commitments: Vec<SerializableG1Affine>,
    ) -> Result<()> {
        let raw_commitments: Vec<G1Affine> = commitments.into_iter().map(|c| c.0).collect();
        let id_parsed = self.id.parse::<usize>().unwrap_or(usize::MAX);

        if self.verify_shares(&shares, &raw_commitments) {
            let my_share = shares.into_iter().find(|s| s.index == id_parsed);
            if let Some(share) = my_share {
                self.shares.write().await.insert(from.clone(), share);
                self.commitments.write().await.insert(from.clone(), raw_commitments);
                
                let validation_msg = DKGMessage::ShareValidation {
                    from: self.id.clone(),
                    to: from,
                    is_valid: true,
                };
                let msg_bytes = serde_json::to_vec(&validation_msg)?;
                self.broadcast(MSG_TOPIC_DKG, &msg_bytes).await?;
            }
        }
        Ok(())
    }

    pub fn verify_share(&self, share: &SerializableScalar, commitments: &[SerializableG1Affine]) -> bool {
        // Implement share verification logic using commitments
        true // Placeholder
    }

    pub fn verify_shares(&self, shares: &[KeyShare], commitments: &[G1Affine]) -> bool {
        // Implement share verification logic using commitments
        true // Placeholder
    }

    async fn handle_share_validation(
        &mut self,
        from: String,
        to: String,
        is_valid: bool,
    ) -> Result<()> {
        let mut validations = self.validations.write().await;
        validations
            .entry(to)
            .or_insert_with(Vec::new)
            .push(is_valid);
            
        self.check_completion().await?;
        Ok(())
    }

    async fn check_completion(&self) -> Result<()> {
        let validations = self.validations.read().await;
        let shares = self.shares.read().await;
        
        if validations.values().all(|v| v.len() >= self.threshold) 
            && shares.len() >= self.threshold {
            self.generate_group_key().await?;
        }
        Ok(())
    }

    async fn generate_group_key(&self) -> Result<G1Projective> {
        let shares = self.shares.read().await;
        let mut group_key = G1Projective::identity();
        
        for share in shares.values() {
            group_key += G1Projective::generator() * share.value.0;
        }
        
        Ok(group_key)
    }
}