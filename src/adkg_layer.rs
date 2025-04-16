use bls12_381::{G1Affine, G1Projective, Scalar};
use ff::Field;
use group::Group;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use anyhow::{anyhow, Result};
use libp2p::{gossipsub, swarm::Swarm};
use log::info;

use crate::network_layer::MyBehaviour;

// ADKG message types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ADKGMessage {
    ShareProposal {
        sender_id: String,
        shares: Vec<Scalar>,
        commitments: Vec<G1Affine>,
    },
    ShareAck {
        sender_id: String,
        receiver_id: String,
        valid: bool,
    },
}

// ADKG node state
pub struct ADKGNode {
    id: String,
    idx: u64,
    n: usize,
    t: usize,
    secret_key_share: Option<Scalar>,
    public_key: Option<G1Projective>,
    commitments: RwLock<std::collections::HashMap<String, Vec<G1Affine>>>,
    shares: RwLock<std::collections::HashMap<String, Scalar>>,
    acks: RwLock<std::collections::HashMap<String, Vec<(String, bool)>>>,
    rng: OsRng,
}

impl ADKGNode {
    pub fn new(id: String, n: usize, t: usize, idx: u64) -> Self {
        Self {
            id,
            idx,
            n,
            t,
            secret_key_share: None,
            public_key: None,
            commitments: RwLock::new(std::collections::HashMap::new()),
            shares: RwLock::new(std::collections::HashMap::new()),
            acks: RwLock::new(std::collections::HashMap::new()),
            rng: OsRng,
        }
    }

    fn generate_polynomial(&mut self, degree: usize) -> (Vec<Scalar>, Scalar) {
        let mut coeffs = vec![Scalar::random(&mut self.rng)];
        for _ in 1..=degree {
            coeffs.push(Scalar::random(&mut self.rng));
        }
        (coeffs, coeffs[0])
    }

    fn evaluate_polynomial(coeffs: &[Scalar], i: u64) -> Scalar {
        let x = Scalar::from(i);
        let mut result = Scalar::zero();
        for (j, coeff) in coeffs.iter().enumerate() {
            let term = *coeff * x.pow(&[(j as u64)]);
            result += term;
        }
        result
    }

    fn generate_commitments(coeffs: &[Scalar]) -> Vec<G1Affine> {
        coeffs
            .iter()
            .map(|coeff| G1Affine::from(G1Projective::generator() * coeff))
            .collect()
    }

    pub async fn start_adkg(&mut self, swarm: &mut Swarm<MyBehaviour>) -> Result<()> {
        let (coeffs, _secret) = self.generate_polynomial(self.t);
        let commitments = Self::generate_commitments(&coeffs);

        let mut shares = Vec::new();
        for i in 1..=self.n as u64 {
            let share = Self::evaluate_polynomial(&coeffs, i);
            shares.push(share);
        }

        let msg = ADKGMessage::ShareProposal {
            sender_id: self.id.clone(),
            shares,
            commitments,
        };

        let msg_bytes = serde_json::to_vec(&msg)?;
        let topic = gossipsub::IdentTopic::new("/adkg/1.0.0");
        
        swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic, msg_bytes)
            .map_err(|e| anyhow!("Failed to publish message: {}", e))?;

        self.commitments
            .write()
            .await
            .insert(self.id.clone(), commitments);

        Ok(())
    }

    pub async fn handle_message(
        &mut self,
        msg: ADKGMessage,
        swarm: &mut Swarm<MyBehaviour>,
    ) -> Result<()> {
        match msg {
            ADKGMessage::ShareProposal {
                sender_id,
                shares,
                commitments,
            } => {
                if self.verify_commitments(&sender_id, &shares, &commitments).await {
                    if self.idx > 0 && (self.idx as usize) <= shares.len() {
                        self.shares
                            .write()
                            .await
                            .insert(sender_id.clone(), shares[self.idx as usize - 1]);
                        self.commitments
                            .write()
                            .await
                            .insert(sender_id.clone(), commitments);
                    }

                    let ack = ADKGMessage::ShareAck {
                        sender_id: self.id.clone(),
                        receiver_id: sender_id.clone(),
                        valid: true,
                    };
                    let ack_bytes = serde_json::to_vec(&ack)?;
                    let topic = gossipsub::IdentTopic::new("/adkg/1.0.0");
                    swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(topic, ack_bytes)
                        .map_err(|e| anyhow!("Failed to publish ACK: {}", e))?;
                }
            }
            ADKGMessage::ShareAck {
                sender_id,
                receiver_id,
                valid,
            } => {
                if receiver_id == self.id {
                    self.acks
                        .write()
                        .await
                        .entry(sender_id.clone())
                        .or_insert_with(Vec::new)
                        .push((sender_id, valid));
                    self.check_completion().await?;
                }
            }
        }
        Ok(())
    }

    async fn verify_commitments(
        &self,
        sender_id: &str,
        shares: &[Scalar],
        commitments: &[G1Affine],
    ) -> bool {
        let node_index = self.idx;
        if node_index == 0 || node_index as usize > shares.len() {
            return false;
        }
        let share = shares[node_index as usize - 1];
        let mut commitment_sum = G1Projective::identity();
        for (j, commitment) in commitments.iter().enumerate() {
            let x = Scalar::from(node_index);
            let term = G1Projective::from(*commitment) * x.pow(&[(j as u64)]);
            commitment_sum += term;
        }
        let expected = G1Projective::generator() * share;
        commitment_sum == expected
    }

    async fn check_completion(&mut self) -> Result<()> {
        let acks = self.acks.read().await;
        if acks.values().flatten().filter(|(_, valid)| *valid).count() >= self.t {
            let shares = self.shares.read().await;
            let mut secret_key_share = Scalar::zero();
            for share in shares.values() {
                secret_key_share += *share;
            }
            self.secret_key_share = Some(secret_key_share);

            let commitments = self.commitments.read().await;
            let mut public_key = G1Projective::identity();
            for comms in commitments.values() {
                public_key += comms[0].into();
            }
            self.public_key = Some(public_key);

            info!("ADKG completed for node {}. Secret share generated.", self.id);
        }
        Ok(())
    }

    pub async fn reconstruct_secret(shares: Vec<(u64, Scalar)>) -> Scalar {
        let mut secret = Scalar::zero();
        for (i, share) in shares.iter() {
            let mut term = *share;
            for (j, _) in shares.iter().filter(|(j, _)| j != i) {
                let num = Scalar::from(*j);
                let den = Scalar::from(*j) - Scalar::from(*i);
                term *= num * den.invert().unwrap();
            }
            secret += term;
        }
        secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bls12_381::Scalar;
    use rand::thread_rng;

    #[tokio::test]
    async fn test_polynomial_generation() {
        let mut node = ADKGNode::new("test".to_string(), 3, 1, 1);
        let (coeffs, secret) = node.generate_polynomial(2);
        
        assert_eq!(coeffs.len(), 3);
        assert_eq!(secret, coeffs[0]);
    }

    #[tokio::test]
    async fn test_commitment_verification() {
        let mut node = ADKGNode::new("test".to_string(), 3, 1, 1);
        let (coeffs, _) = node.generate_polynomial(2);
        let commitments = ADKGNode::generate_commitments(&coeffs);
        let shares: Vec<Scalar> = (1..=3)
            .map(|i| ADKGNode::evaluate_polynomial(&coeffs, i))
            .collect();

        assert!(node.verify_commitments("test", &shares, &commitments).await);
    }

    #[tokio::test]
    async fn test_secret_reconstruction() {
        let mut node = ADKGNode::new("test".to_string(), 3, 1, 1);
        let (coeffs, secret) = node.generate_polynomial(2);
        
        let shares: Vec<(u64, Scalar)> = (1..=3)
            .map(|i| (i, ADKGNode::evaluate_polynomial(&coeffs, i)))
            .collect();

        let reconstructed = ADKGNode::reconstruct_secret(shares).await;
        assert_eq!(reconstructed, secret);
    }

    #[tokio::test]
    async fn test_share_proposal_handling() {
        let mut node = ADKGNode::new("receiver".to_string(), 3, 1, 1);
        let mut sender = ADKGNode::new("sender".to_string(), 3, 1, 2);
        
        let (coeffs, _) = sender.generate_polynomial(2);
        let commitments = ADKGNode::generate_commitments(&coeffs);
        let shares: Vec<Scalar> = (1..=3)
            .map(|i| ADKGNode::evaluate_polynomial(&coeffs, i))
            .collect();

        let msg = ADKGMessage::ShareProposal {
            sender_id: "sender".to_string(),
            shares,
            commitments,
        };

        // Create a dummy swarm for testing
        use libp2p::{identity, swarm::Swarm};
        let key = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(key.public());
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&key).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();
        
        let gossipsub_config = gossipsub::ConfigBuilder::default().build().unwrap();
        let mut swarm = Swarm::new(
            transport,
            MyBehaviour {
                gossipsub: gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key),
                    gossipsub_config,
                ).unwrap(),
                mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id).unwrap(),
            },
            peer_id,
        );

        let result = node.handle_message(msg, &mut swarm).await;
        assert!(result.is_ok());
        
        let shares = node.shares.read().await;
        assert!(shares.contains_key("sender"));
    }
}