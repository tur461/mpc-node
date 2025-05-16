use std::{
    any::Any, collections::{HashMap, HashSet}, env, fmt::Debug, time::Duration
};
use std::{
    collections::hash_map::DefaultHasher, hash::{Hash, Hasher}
};
use anyhow::Result;
use futures::StreamExt;
use libp2p::{
    gossipsub::{self, Message, TopicHash}, identity::Keypair, mdns::{self}, noise, request_response::{cbor::Behaviour as CborReqRespBehaviour, Event as ReqRespEvent, Message as ReqRespMessage}, swarm::{ConnectionError, NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, Swarm, Transport
};
use tokio::sync::{mpsc, RwLock,};
use tokio::{io, io::AsyncBufReadExt, select, io::Error};
use tonic::transport::Server;
use tracing::{debug, info, warn};

use crate::{dkg::DKGStatus, protos::intent::intent_service_server::IntentServiceServer, types::{dkg::MSG_TOPIC_DKG, p2p::SerializablePeerId, rpc::{build_mpc_behaviour, MPCCodec, MPCCodecRequest, MPCCodecResponse, MPCProtocol, RPCRequest, RPCResponse}, GossipsubMessage, ChannelMessage, PeerInfo}};
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
    message_tx: mpsc::Sender<ChannelMessage>,
    message_rx: mpsc::Receiver<ChannelMessage>,
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

    pub fn get_msg_tx(&self) -> mpsc::Sender<ChannelMessage> {
        self.message_tx.clone()
    }

    pub fn get_msg_rx(&mut self) -> &mut mpsc::Receiver<ChannelMessage> {
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

        let args: Vec<String> = env::args().collect();
        if args.len() >= 2 {
            // eprintln!("Usage: {} <multiaddr>", args[0]);
            // std::process::exit(1);
            let server_addr: Multiaddr = args[1].parse()?;
            println!("Dialing address: {}", server_addr);
            swarm.dial(server_addr.clone())?;
        }

        println!("Local Peer ID: {}", self.local_peer_id);

        self.subscribe_to_default_topics(&mut swarm)?;
        
        loop {
            tokio::select! {
                event = swarm.select_next_some() => {
                    info!("[SWARM] some");
                    self.handle_swarm_event(event, &mut swarm).await?;
                }
                // only hit when there is anything on the mpsc channel
                msg = self.message_rx.recv() => {
                    if let Some(msg) = msg {
                        // lets handle the channel msg on the basis of its type
                        self.handle_channel_message(msg, &mut swarm).await?;
                    }
                }
            };
        }
    }

    fn subscribe_to_default_topics(&mut self, swarm: &mut libp2p::Swarm<MPCBehaviour>) -> Result<()> {
        let topics = vec![MSG_TOPIC_DKG, "signing", "consensus", "commands"];
        for topic in topics {
            let topic_hash = gossipsub::IdentTopic::new(topic);
            info!("topic hash for {} is {}", topic, topic_hash);
            swarm.behaviour_mut().gossipsub.subscribe(&topic_hash)?;
            self.topics.insert(topic_hash.hash());
        }
        Ok(())
    }

    async fn handle_swarm_event(&mut self, event: SwarmEvent<NetworkEvent>, swarm: &mut libp2p::Swarm<MPCBehaviour>) -> Result<()> {
        info!("[SWARM EVENT] {:?}", event);
        match event {
            // if the event is of type RequestResponse
            // and depending upon the nature like:
            // if the branch is Response, the peer can send the type Request to other peers
            // else type Response will be sent to other peers
            // someone will initiate the process by sending a NetworkEvent::RequestResponse with msg type ReqRespMessage::Request
            // to other peer(s)
            // the other peer(s) will recieve the it as ReqRespMessage::Request under the event NetworkEvent::RequestResponse
            // the reciever of the init will be one or many depending on how it was send
            // if sent by calling on peer_id, the reciever will be one; if called on swarm, it'll be a broadcast
            SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqRespEvent::Message { peer, message, .. })) => { // peer.r
                info!("RPC request: peer: {} response: {:?}",peer, message);
                match message {
                    // this peer will recieve a request from other peer(s) (incl testing agent(s))
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
                            RPCRequest::StartADKG// {n, threshold } 
                            => {
                                // handle dkg start
                                match self.dkg_node.as_mut() {
                                    Some(dkg) => {
                                        dkg.start_dkg().await?;
                                        RPCResponse::DKGStarted {peer_id: SerializablePeerId(peer)}
                                    }
                                    None => RPCResponse::DKGError("dkg is not initialized.".to_string()),
                                }
                            },
                            // This peer node will recieve a request from some external agent that will
                            // connect with this peer to request it to initiate dkg
                            RPCRequest::GetDKGStatus => { // peer.r.dkg.ptp
                                let status = self
                                    .dkg_node
                                    .as_ref()
                                    .map(|dkg| dkg.get_status())
                                    .unwrap_or(DKGStatus::New);

                                RPCResponse::DKGStatus { status } // send back to the agent at peer.send.ptp
                            },
                            RPCRequest::SendAVSSShare { dealer_id, share, commitments, .. } => {
                                if self.dkg_node.is_none() {
                                    RPCResponse::Error("DKG not initialized".to_string())
                                } else if self.dkg_node.as_ref().unwrap().verify_share(&share, &commitments) {
                                    RPCResponse::ShareAccepted
                                } else {
                                    RPCResponse::Error("Invalid share".into())
                                }
                            },
                            _ => RPCResponse::Ack
                        };
                        // here the following line will send to the other connected peers (incl any testing agent(s))
                        let _ = swarm.behaviour_mut().request_response.send_response(channel, response); // peer.send.ptp
                    },
                    // here peer(s) will recieve the msg sent by other peer by peer.send.ptp
                    ReqRespMessage::Response { request_id, response } => { // peer.r
                        match response {
                            RPCResponse::GotAVSSShare { dealer_id, receiver_id, share, commitments } => {
                                // Handle AVSS share response here (maybe store in state?)
                                // e.g., self.dkg_node.as_mut()?.handle_received_share(party_id, share, commitments);
                                
                            }
            
                            RPCResponse::DKGStarted {peer_id} => {
                                // dkg started by sending peer
                                info!("DKG has started by: {:?}", peer_id);
                            }
            
                            RPCResponse::Error(err) => {
                                // error received because of previous sent request
                                warn!("Received error from peer {}: {}", peer, err);
                            }

                            RPCResponse::ShareAccepted => {
                                // share accepted by the peer whom this node sent it to

                            }
            
                            _ => {
                                // Optionally handle other responses like DKGStatus, ShareAccepted, etc.
                            }
                        }
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
            // if the msg sent by a peer has event type of Gossipsub, this branch will be hit on the 
            // reciever(s)
            SwarmEvent::Behaviour(NetworkEvent::Gossipsub(gossipsub::Event::Message { // dkg.r.2.0
                message_id,
                propagation_source,
                message,
            })) => {
                info!(
                    "Received message with id: {} from: {}",
                    message_id, propagation_source
                );
                self.handle_gossipsub_message(&message).await?;
            },
            SwarmEvent::Behaviour(NetworkEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic })) => {
                info!("Subscribed to topic: {}", topic);
            },
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
        info!("i am at handle_gossipsub_message ---------- {:?}", message);
        let res = serde_json::from_slice::<GossipsubMessage>(&message.data); 
        if let Err(e) = &res  {
            info!("now I know the error ----- {:?}", e);
        }
        let msg = res.unwrap();
        info!("[GMSG] {:?}", msg);
        match msg {
            // this must be used by broadcast in dkg (sending end)
            // so, here it must match on the recieving end
            GossipsubMessage::DKG(dkg_msg) => { // dkg.r.2.1
                // here we unwrapped the outer gossipmsg wrap from 
                // the msg bytes (check broadcast_shares() in dkg module)
                if let Some(dkg_node) = &mut self.dkg_node {
                    // now we can unwrap the internal dkg wrap to get the DKG related things
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

    // ChannelMessage is related to the MPSC channel only
    // the channel is setup between 
    // 1. dkg thread (writing end) and tokio async task executor (reading end)
    async fn handle_channel_message(
        &mut self,
        message: ChannelMessage,
        swarm: &mut libp2p::Swarm<MPCBehaviour>,
    ) -> Result<()> {
        info!("[handle_channel_message()] {:?}", message);
        match message {
            // this is used in the dkg code
            ChannelMessage::Broadcast { topic, data } => { // dkg.s.1
                info!("[handle_channel_message()] [BCAST] topic: {}", topic);
                let topic_hash = gossipsub::IdentTopic::new(topic);
                // here the channel msg is sent to other peer(s) as Gossipsub MSG type
                // if peer(s) have already subscribed to this topic, they will recieve this 
                // msg (check subscribe_to_default_topics())
                let swarm_mut = swarm.behaviour_mut();
                info!("[handle_channel_message()] [BCAST] prs count: {}", swarm_mut.gossipsub.all_peers().count());
                if let Err(e) = swarm_mut.gossipsub.publish(topic_hash, data) {
                    info!("error: {:?}",e);
                };
            }
            // this is not used anywhere yet
            ChannelMessage::Unicast { peer_id, data } => {
                // Implement direct message sending using request-response protocol
                let peer_id = PeerId::from_bytes(&hex::decode(peer_id)?)
                    .map_err(|e| anyhow::anyhow!("Invalid peer ID: {}", e))?;
                let request = RPCRequest::Custom {
                    id: "custom".to_string(),
                    data,
                };
                // this will send the channel msg as 
                // NetworkEvent::RequestResponse under ReqRespMessage::Request type
                // to just single peer
                swarm.behaviour_mut().request_response.send_request(&peer_id, request);
            
            }
        }
        Ok(())
    }

    pub async fn broadcast(&self, topic: &str, data: &[u8]) -> Result<()> {
        self.message_tx.send(ChannelMessage::Broadcast {
            topic: topic.to_string(),
            data: data.to_vec(),
        }).await?;
        Ok(())
    }

    pub async fn send_direct_message(&self, peer_id: &str, data: &[u8]) -> Result<()> {
        self.message_tx.send(ChannelMessage::Unicast {
            peer_id: peer_id.to_string(),
            data: data.to_vec(),
        }).await?;
        Ok(())
    }
}
