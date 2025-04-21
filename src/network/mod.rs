use std::{
    any::Any, collections::{HashMap, HashSet}, fmt::Debug, time::Duration
};
use std::{
    collections::hash_map::DefaultHasher, hash::{Hash, Hasher}
};
use anyhow::Result;
use futures::StreamExt;
use libp2p::{
    gossipsub::{self, Message, TopicHash}, identity::Keypair, mdns::{self}, noise, request_response::{cbor::Behaviour as CborReqRespBehaviour, Event as ReqRespEvent, Message as ReqRespMessage}, swarm::{ConnectionError, NetworkBehaviour, SwarmEvent}, tcp, yamux, PeerId, Swarm, Transport
};
use tokio::sync::{mpsc, RwLock,};
use tokio::{io, io::AsyncBufReadExt, select, io::Error};
use tonic::transport::Server;
use tracing::{debug, info, warn};

use crate::{dkg::DKGStatus, protos::intent::intent_service_server::IntentServiceServer, types::{rpc::{build_mpc_behaviour, MPCCodec, MPCCodecRequest, MPCCodecResponse, MPCProtocol, RPCRequest, RPCResponse}, GossipsubMessage, NetworkMessage, PeerInfo}};
use crate::dkg::DKGNode;
use crate::signing::SigningNode;
use crate::consensus::ConsensusNode;
use crate::commands::CommandProcessor;
use crate::protos::intent::{IntentRequest, intent_service_server::IntentService};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "NetworkEvent")]
pub struct MPCBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
    pub request_response: CborReqRespBehaviour<MPCCodecRequest, MPCCodecResponse>
}

#[derive(Debug)]
pub enum NetworkEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    RequestResponse(ReqRespEvent<RPCRequest, RPCResponse>),
}

impl From<gossipsub::Event> for NetworkEvent {
    fn from(event: gossipsub::Event) -> Self {
        NetworkEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for NetworkEvent {
    fn from(event: mdns::Event) -> Self {
        NetworkEvent::Mdns(event)
    }
}

impl From<ReqRespEvent<RPCRequest, RPCResponse>> for NetworkEvent {
    fn from(event: ReqRespEvent<RPCRequest, RPCResponse>) -> Self {
        NetworkEvent::RequestResponse(event)
    }
}

pub struct NetworkLayer {
    local_key: Keypair,
    local_peer_id: PeerId,
    peers: RwLock<HashMap<PeerId, PeerInfo>>,
    message_tx: mpsc::Sender<NetworkMessage>,
    message_rx: mpsc::Receiver<NetworkMessage>,
    topics: HashSet<TopicHash>,
    dkg_node: Option<DKGNode>,
    signing_node: Option<SigningNode>,
    consensus_node: Option<ConsensusNode>,
    command_processor: Option<CommandProcessor>,
}

impl NetworkLayer {
    pub async fn new() -> Result<Self> {
        let local_key = Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        let (tx, rx) = mpsc::channel(100);

        Ok(Self {
            local_key,
            local_peer_id,
            peers: RwLock::new(HashMap::new()),
            message_tx: tx,
            message_rx: rx,
            topics: HashSet::new(),
            dkg_node: None,
            signing_node: None,
            consensus_node: None,
            command_processor: None,
        })
    }

    pub fn set_dkg_node(&mut self, dkg_node: DKGNode) {
        self.dkg_node = Some(dkg_node);
    }

    pub fn set_signing_node(&mut self, signing_node: SigningNode) {
        self.signing_node = Some(signing_node);
    }

    pub fn set_consensus_node(&mut self, node: ConsensusNode) {
        self.consensus_node = Some(node);
    }

    pub fn set_command_processor(&mut self, processor: CommandProcessor) {
        self.command_processor = Some(processor);
    }

    pub fn get_msg_tx(&self) -> mpsc::Sender<NetworkMessage> {
        self.message_tx.clone()
    }

    pub fn get_msg_rx(&mut self) -> &mut mpsc::Receiver<NetworkMessage> {
        &mut self.message_rx
    }

    pub fn get_local_peer_id<'a>(&'a self) -> &'a PeerId {
        &self.local_peer_id
    }

    pub async fn start(&mut self) -> Result<()> {
        
        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(self.local_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(30)) 
                .validation_mode(gossipsub::ValidationMode::Strict)
                .message_id_fn(message_id_fn) 
                .build()
                .map_err(io::Error::other).map_err(io::Error::other)?;

            
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns_config = mdns::Config {
                query_interval: Duration::from_secs(10),
                ttl: Duration::from_secs(10),
                ..Default::default()
            };

            let mdns =
                mdns::tokio::Behaviour::new(mdns_config, key.public().to_peer_id())?;

            let request_response = build_mpc_behaviour();

            Ok(MPCBehaviour {
                gossipsub,
                mdns,
                request_response,
            })
        })?
        .build();

        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

        println!("Local Peer ID: {}", self.local_peer_id);

        self.subscribe_to_default_topics(&mut swarm)?;
        
        loop {
            tokio::select! {
                event = swarm.select_next_some() => {
                    info!("[SWARM] some");
                    self.handle_swarm_event(event, &mut swarm).await?;
                }
                msg = self.message_rx.recv() => {
                    if let Some(msg) = msg {
                        self.handle_network_message(msg, &mut swarm).await?;
                    }
                }
            };
        }
    }

    fn subscribe_to_default_topics(&mut self, swarm: &mut libp2p::Swarm<MPCBehaviour>) -> Result<()> {
        let topics = vec!["dkg", "signing", "consensus", "commands"];
        for topic in topics {
            let topic_hash = gossipsub::IdentTopic::new(topic);
            swarm.behaviour_mut().gossipsub.subscribe(&topic_hash)?;
            self.topics.insert(topic_hash.hash());
        }
        Ok(())
    }

    async fn handle_swarm_event(&mut self, event: SwarmEvent<NetworkEvent>, swarm: &mut libp2p::Swarm<MPCBehaviour>) -> Result<()> {
        match event {
            SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqRespEvent::Message { peer, message, .. })) => {
                info!("RPC request: peer: {} response: {:?}",peer, message);
                match message {
                    ReqRespMessage::Request { request, channel, .. } => {
                        let response = match request {
                            RPCRequest::Ping => RPCResponse::Pong,
                            RPCRequest::GetPeerInfo => {
                                // Provide your implementation to get peer info
                                RPCResponse::PeerInfo(PeerInfo {
                                    addresses: vec![],
                                    connected: true,
                                })
                            }
                            RPCRequest::Custom { id, data } => {
                                // Handle custom request
                                RPCResponse::Custom { id, data }
                            },
                            RPCRequest::StartDKG => {
                                // handle dkg start
                                match self.dkg_node.as_mut() {
                                    Some(dkg) => {
                                        dkg.start_dkg().await?;
                                        RPCResponse::DKGStarted
                                    }
                                    None => RPCResponse::DKGError("dkg is not initialized.".to_string()),
                                }
                            },
                            RPCRequest::GetDKGStatus => {
                                let status = self
                                    .dkg_node
                                    .as_ref()
                                    .map(|dkg| dkg.get_status())
                                    .unwrap_or(DKGStatus::New);

                                RPCResponse::DKGStatus { status }
                            },
                        };
                        let _ = swarm.behaviour_mut().request_response.send_response(channel, response);
                    },
                    ReqRespMessage::Response { response, .. } => {
                        // here we need to handle RPC response
                        println!("RPC response: response: {:?}",response);
                        info!("Received response: {:?}", response);
                    }
                    
                }
            }
            SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqRespEvent::OutboundFailure { peer, error, .. })) => {
                info!("Outbound failure to peer {}: {:?}", peer, error);
            }
            SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqRespEvent::InboundFailure { peer, error, .. })) => {
                info!("Inbound failure to peer {}: {:?}", peer, error);
            }
            SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqRespEvent::ResponseSent { peer, .. })) => {
                info!("Response sent to peer {}", peer);
            }
            SwarmEvent::Behaviour(NetworkEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer_id, addr) in peers {
                    info!("Discovered peer: {peer_id} at {addr}");
                    self.peers.write().await.insert(peer_id, PeerInfo {
                        addresses: vec![addr],
                        connected: true,
                    });
                }
                println!("[NEW] Peers: {}", self.peers.read().await.values().filter(|peer| peer.connected).collect::<Vec<_>>().len());
            }
            SwarmEvent::Behaviour(NetworkEvent::Mdns(mdns::Event::Expired(peers))) => {
                info!("[EXP]");
                for (peer_id, _) in peers {
                    if let Some(peer) = self.peers.write().await.get_mut(&peer_id) {
                        peer.connected = false;
                    }
                }
                println!("[EXP] Peers: {}", self.peers.read().await.values().filter(|peer| peer.connected).collect::<Vec<_>>().len());
            }
            SwarmEvent::Behaviour(NetworkEvent::Gossipsub(gossipsub::Event::Message { 
                message_id,
                propagation_source,
                message,
            })) => {
                info!(
                    "Received message with id: {} from: {}",
                    message_id, propagation_source
                );
                self.handle_gossipsub_message(&message).await?;
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
                info!("Listening for RPC connections on {}{}", address, MPCProtocol.as_ref());
            }
            SwarmEvent::ConnectionClosed { peer_id, connection_id, endpoint, num_established, cause } => {
                if let Some(ca) = cause {
                    match ca {
                        ConnectionError::IO(e) => {
                            info!("[ERROR] Kind: {} RAW: {:?} ref: {:?} Full: {}", e.kind(), e.raw_os_error(), e.get_ref().as_slice(), e);
                        }
                        _ => {}
                    }
                }
            }
                
            _ => {
                info!("[EVENT] other");
            }
        }
        Ok(())
    }

    async fn handle_gossipsub_message(&mut self, message: &Message) -> Result<()> {
        let msg: GossipsubMessage = serde_json::from_slice(&message.data)?;
        
        match msg {
            GossipsubMessage::DKG(dkg_msg) => {
                if let Some(dkg_node) = &mut self.dkg_node {
                    dkg_node.handle_message(dkg_msg).await?;
                }
            }
            GossipsubMessage::Signing(signing_msg) => {
                if let Some(signing_node) = &mut self.signing_node {
                    signing_node.handle_message(signing_msg).await?;
                }
            }
            GossipsubMessage::Command(cmd) => {
                if let Some(cmd_processor) = &mut self.command_processor {
                    cmd_processor.handle_message(cmd).await?;
                }
            }
            GossipsubMessage::Consensus(consensus_msg) => {
                if let Some(consensus_node) = &mut self.consensus_node {
                    consensus_node.handle_message(consensus_msg).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_network_message(
        &mut self,
        message: NetworkMessage,
        swarm: &mut libp2p::Swarm<MPCBehaviour>,
    ) -> Result<()> {
        info!("[NW MSG] {:?}", message);
        match message {
            NetworkMessage::Broadcast { topic, data } => {
                let topic_hash = gossipsub::IdentTopic::new(topic);
                swarm.behaviour_mut().gossipsub.publish(topic_hash, data)?;
            }
            NetworkMessage::DirectMessage { peer_id, data } => {
                // Implement direct message sending using request-response protocol
                let peer_id = PeerId::from_bytes(&hex::decode(peer_id)?)
                    .map_err(|e| anyhow::anyhow!("Invalid peer ID: {}", e))?;
                let request = RPCRequest::Custom {
                    id: "custom".to_string(),
                    data,
                };
                swarm.behaviour_mut().request_response.send_request(&peer_id, request);
            
            }
        }
        Ok(())
    }

    pub async fn broadcast(&self, topic: &str, data: &[u8]) -> Result<()> {
        self.message_tx.send(NetworkMessage::Broadcast {
            topic: topic.to_string(),
            data: data.to_vec(),
        }).await?;
        Ok(())
    }

    pub async fn send_direct_message(&self, peer_id: &str, data: &[u8]) -> Result<()> {
        self.message_tx.send(NetworkMessage::DirectMessage {
            peer_id: peer_id.to_string(),
            data: data.to_vec(),
        }).await?;
        Ok(())
    }
}