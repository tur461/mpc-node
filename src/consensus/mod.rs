use blake3::Hash;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrfOutput {
    pub proof: Vec<u8>,
    pub hash: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Proposal {
        round: u64,
        value: Vec<u8>,
        vrf_proof: VrfOutput,
    },
    Vote {
        round: u64,
        proposal_hash: Hash,
        voter: String,
    },
}

pub struct ConsensusNode {
    vrf_secret_key: [u8; 32],
    vrf_public_key: [u8; 32],
    state: HashMap<String, Vec<u8>>,
    current_round: u64,
    proposals: HashMap<u64, Vec<(Vec<u8>, VrfOutput)>>,
    votes: HashMap<u64, HashMap<Hash, Vec<String>>>,
}

impl ConsensusNode {
    pub fn new() -> Self {
        let mut rng = OsRng;
        let mut secret_key = [0u8; 32];
        rng.fill_bytes(&mut secret_key);
        
        let public_key = blake3::keyed_hash(&secret_key, b"vrf-public-key").as_bytes().clone();
        
        Self {
            vrf_secret_key: secret_key,
            vrf_public_key: public_key,
            state: HashMap::new(),
            current_round: 0,
            proposals: HashMap::new(),
            votes: HashMap::new(),
        }
    }

    pub fn generate_vrf_proof(&self, input: &[u8]) -> VrfOutput {
        let proof = blake3::keyed_hash(&self.vrf_secret_key, input).as_bytes().to_vec();
        let hash = blake3::hash(input);
        
        VrfOutput { proof, hash }
    }

    pub fn verify_vrf_proof(&self, input: &[u8], output: &VrfOutput) -> bool {
        let expected_hash = blake3::hash(input);
        output.hash == expected_hash
    }

    pub fn update_state(&mut self, key: String, value: Vec<u8>) {
        self.state.insert(key, value);
    }

    pub async fn handle_message(&self, msg: ConsensusMessage) -> Result<()> {
        match msg {
            ConsensusMessage::Proposal { round, value, vrf_proof } => {
                self.handle_proposal(round, value, vrf_proof).await?;
            }
            ConsensusMessage::Vote { round, proposal_hash, voter } => {
                self.handle_vote(round, proposal_hash, voter).await?;
            }
        }
        Ok(())
    }

    async fn handle_proposal(
        &self,
        round: u64,
        value: Vec<u8>,
        vrf_proof: VrfOutput,
    ) -> Result<()> {
        // Verify VRF proof and process proposal
        if self.verify_vrf_proof(&value, &vrf_proof) {
            // Add to proposals and potentially broadcast vote
        }
        Ok(())
    }

    async fn handle_vote(
        &self,
        round: u64,
        proposal_hash: Hash,
        voter: String,
    ) -> Result<()> {
        // Process vote and check for consensus
        Ok(())
    }

    pub fn get_state(&self, key: &str) -> Option<&Vec<u8>> {
        self.state.get(key)
    }
}