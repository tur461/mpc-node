use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{Hash, Hasher},
    time::Duration,
};
use futures::stream::StreamExt;
use libp2p::{
    gossipsub, mdns, noise,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId, Transport,
};
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;

use crate::adkg_layer::{ADKGMessage, ADKGNode};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyBehaviourEvent")]
pub struct MyBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

#[derive(Debug)]
pub enum MyBehaviourEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
}

impl From<gossipsub::Event> for MyBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        MyBehaviourEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for MyBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        MyBehaviourEvent::Mdns(event)
    }
}

pub async fn run(n: usize, t: usize, idx: u64) -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .map_err(|e| Box::new(e) as Box<dyn Error>)?;

    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    let node_id = local_peer_id.to_string();

    // Create TCP transport
    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default())
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .boxed();

    // Create QUIC transport
    let quic_transport = libp2p::quic::tokio::Transport::new(libp2p::quic::Config::new(&local_key));

    // Combine transports
    let transport = libp2p::core::transport::OrTransport::new(quic_transport, tcp_transport)
        .boxed();

    // Create gossipsub configuration
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
        .map_err(|e| Box::new(e) as Box<dyn Error>)?;

    // Create swarm
    let mut swarm = SwarmBuilder::with_tokio_executor(
        transport,
        MyBehaviour {
            gossipsub: gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(local_key.clone()),
                gossipsub_config,
            )?,
            mdns: mdns::tokio::Behaviour::new(
                mdns::Config {
                    query_interval: Duration::from_secs(10),
                    ttl: Duration::from_secs(10),
                    ..Default::default()
                },
                local_peer_id,
            )?,
        },
        local_peer_id,
    )
    .build();

    // Subscribe to topics
    let topic = gossipsub::IdentTopic::new("test-net");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    let adkg_topic = gossipsub::IdentTopic::new("/adkg/1.0.0");
    swarm.behaviour_mut().gossipsub.subscribe(&adkg_topic)?;

    // Set up listeners
    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let mut stdin = io::BufReader::new(io::stdin()).lines();
    let mut peer_list: Vec<PeerId> = Vec::new();
    let mut adkg_node = ADKGNode::new(node_id.clone(), n, t, idx);

    println!("Peer ID: {}", swarm.local_peer_id());
    adkg_node.start_adkg(&mut swarm).await?;

    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if let Err(e) = swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(topic.clone(), line.as_bytes())
                {
                    println!("Publish error: {e:?}");
                }
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discovered a new peer: {peer_id}");
                        peer_list.retain(|p| p != &peer_id);
                        peer_list.push(peer_id);
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                        println!("[NEW] Peers: {}", peer_list.len());
                    }
                }
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discover peer has expired: {peer_id}");
                        peer_list.retain(|p| p != &peer_id);
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                        println!("[EXP] Peers: {}", peer_list.len());
                    }
                }
                SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                })) => {
                    if let Ok(adkg_msg) = serde_json::from_slice::<ADKGMessage>(&message.data) {
                        adkg_node.handle_message(adkg_msg, &mut swarm).await?;
                    } else {
                        println!(
                            "Got non-ADKG message: '{}' with id: {id} from peer: {peer_id}",
                            String::from_utf8_lossy(&message.data)
                        );
                    }
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Local node is listening on {address}");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_swarm_initialization() {
        let result = run(3, 1, 1).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_topic_subscription() {
        let key = Keypair::generate_ed25519();
        let peer_id = PeerId::from(key.public());
        
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&key).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .build()
            .unwrap();

        let mut swarm = SwarmBuilder::with_tokio_executor(
            transport,
            MyBehaviour {
                gossipsub: gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key),
                    gossipsub_config,
                ).unwrap(),
                mdns: mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    peer_id,
                ).unwrap(),
            },
            peer_id,
        )
        .build();

        let topic = gossipsub::IdentTopic::new("test-net");
        assert!(swarm.behaviour_mut().gossipsub.subscribe(&topic).is_ok());
    }

    #[tokio::test]
    async fn test_message_publishing() {
        let key = Keypair::generate_ed25519();
        let peer_id = PeerId::from(key.public());

        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&key).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .build()
            .unwrap();

        let mut swarm = SwarmBuilder::with_tokio_executor(
            transport,
            MyBehaviour {
                gossipsub: gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key),
                    gossipsub_config,
                ).unwrap(),
                mdns: mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    peer_id,
                ).unwrap(),
            },
            peer_id,
        )
        .build();

        let topic = gossipsub::IdentTopic::new("test-net");
        swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();
        
        let publish_result = swarm.behaviour_mut()
            .gossipsub
            .publish(topic, b"test message".to_vec());
        
        assert!(publish_result.is_ok());
    }
}