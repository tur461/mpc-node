use bls12_381::{G1Affine, G1Projective, Scalar};
use ff::Field;
use group::Group;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use crate::network_layer_bkp::MyBehaviour;
use anyhow::{anyhow, Result};
use libp2p::{gossipsub, swarm::NetworkBehaviour};
use futures::StreamExt;

// ADKG message types
#[derive(Serialize, Deserialize, Clone, Debug)]
enum ADKGMessage {
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
struct ADKGNode {
    id: String,
    idx: u64,
    n: usize, // Total number of nodes
    t: usize, // Threshold
    secret_key_share: Option<Scalar>,
    public_key: Option<G1Projective>,
    commitments: RwLock<std::collections::HashMap<String, Vec<G1Affine>>>,
    shares: RwLock<std::collections::HashMap<String, Scalar>>,
    acks: RwLock<std::collections::HashMap<String, Vec<(String, bool)>>>,
    rng: OsRng,
}

impl ADKGNode {
    fn new(id: String, idx: u64, n: usize, t: usize) -> Self {
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

    // Generate polynomial for secret sharing
    fn generate_polynomial(&mut self, degree: usize) -> (Vec<Scalar>, Scalar) {
        let mut coeffs = vec![Scalar::random(&mut self.rng)];
        for _ in 1..=degree {
            coeffs.push(Scalar::random(&mut self.rng));
        }
        (coeffs, coeffs[0]) // Secret is the constant term
    }

    // Evaluate polynomial at index i
    fn evaluate_polynomial(coeffs: &[Scalar], i: u64) -> Scalar {
        let x = Scalar::from(i);
        let mut result = Scalar::zero();
        for (j, coeff) in coeffs.iter().enumerate() {
            let term = *coeff * x.pow(&[(j as u64)]);
            result += term;
        }
        result
    }

    // Generate commitments for polynomial coefficients
    fn generate_commitments(coeffs: &[Scalar]) -> Vec<G1Affine> {
        coeffs
            .iter()
            .map(|coeff| G1Affine::from(G1Projective::generator() * coeff))
            .collect()
    }

    // Initiate key share generation
    async fn start_adkg(&mut self, swarm: &mut libp2p::Swarm<impl NetworkBehaviour>) -> Result<()> {
        let (coeffs, _secret): (Vec<Scalar>, Scalar) = self.generate_polynomial(self.t);
        let commitments = Self::generate_commitments(&coeffs.to_vec());

        // Generate shares for each node (1 to n)
        let mut shares = Vec::new();
        for i in 1..=self.n as u64 {
            let share = Self::evaluate_polynomial(&coeffs.to_vec(), i);
            shares.push(share);
        }

        // Broadcast share proposal
        let msg = ADKGMessage::ShareProposal {
            sender_id: self.id.clone(),
            shares,
            commitments: commitments.clone(),
        };
        let msg_bytes = serde_json::to_vec(&msg)?;
        let topic = gossipsub::IdentTopic::new("/adkg/1.0.0");
        swarm
            .behaviour_mut()
            //.nth(0)
            .unwrap()
            .publish(topic, msg_bytes)
            .map_err(|e| anyhow!("Failed to publish message: {}", e))?;

        // Store own commitments
        self.commitments
            .write()
            .await
            .insert(self.id.clone(), commitments);

        Ok(())
    }

    // Handle incoming ADKG messages
    async fn handle_message(
        &mut self,
        msg: ADKGMessage,
        swarm: &mut libp2p::Swarm<MyBehaviour>,
    ) -> Result<()> {
        match msg {
            ADKGMessage::ShareProposal {
                sender_id,
                shares,
                commitments,
            } => {
                // Verify commitments and store share
                if self.verify_commitments(&sender_id, &shares.to_vec(), &commitments.to_vec()).await {
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

                    // Send ACK
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

    // Verify commitments for received shares
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

    // Check if ADKG is complete
    async fn check_completion(&mut self) -> Result<()> {
        let acks = self.acks.read().await;
        if acks.values().flatten().filter(|(_, valid)| *valid).count() >= self.t {
            // Aggregate shares
            let shares = self.shares.read().await;
            let mut secret_key_share = Scalar::zero();
            for share in shares.values() {
                secret_key_share += *share;
            }
            self.secret_key_share = Some(secret_key_share);

            // Compute public key
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
    // Helper for testing: reconstruct secret from shares
    async fn reconstruct_secret(shares: Vec<(u64, Scalar)>) -> Scalar {
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
