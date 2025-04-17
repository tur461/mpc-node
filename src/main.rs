use std::error::Error;
use tracing_subscriber::EnvFilter;
use mpc_node::{
    network::NetworkLayer,
    dkg::DKGNode,
    consensus::ConsensusNode,
    commands::CommandProcessor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let threshold = 3;
    let total = 5;

    let mut network = NetworkLayer::new().await?;
    
    let dkg_node = DKGNode::new(
        network.get_local_peer_id().to_string(),
        threshold,
        total,
        network.get_msg_tx(),
    );
    
    let consensus_node = ConsensusNode::new();
    let command_processor = CommandProcessor::new();
    
    network.set_dkg_node(dkg_node);
    network.set_consensus_node(consensus_node);
    network.set_command_processor(command_processor);
    
    // Start the network
    network.start().await?;

    Ok(())
}