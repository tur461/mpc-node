use libp2p::{
    identity::{self, Keypair}, noise, request_response::{cbor::Behaviour as CborReqRespBehaviour, Event as ReqRespEvent, Message as ReqRespMsg, ProtocolSupport }, swarm::{dial_opts::DialOpts, NetworkBehaviour}, tcp, yamux, Multiaddr, PeerId, Swarm, Transport
};
use mpc_node::types::rpc::{build_mpc_behaviour, MPCCodec, MPCCodecRequest, MPCCodecResponse, RPCRequest, RPCResponse};
use serde::{Deserialize, Serialize};
use std::{env, error::Error};
use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyBehaviourEvent")]
struct MyBehaviour {
    pub request_response: CborReqRespBehaviour<MPCCodecRequest, MPCCodecResponse>
    // other behaviours..
}

#[derive(Debug)]
enum MyBehaviourEvent {
    RequestResponse(ReqRespEvent<RPCRequest, RPCResponse>),
    // other events..
}

impl From<ReqRespEvent<RPCRequest, RPCResponse>> for MyBehaviourEvent {
    fn from(event: ReqRespEvent<RPCRequest, RPCResponse>) -> Self {
        MyBehaviourEvent::RequestResponse(event)
    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    let local_key = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    if args.len() < 2 {
        eprintln!("Usage: {} <multiaddr>", args[0]);
        std::process::exit(1);
    }

    let server_addr: Multiaddr = args[1].parse()?;
    println!("Dialing address: {}", server_addr);

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            let request_response = build_mpc_behaviour();
            Ok(MyBehaviour {request_response})
        })?
        .build();
    swarm.dial(server_addr.clone())?;
    

    

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::ConnectionEstablished { peer_id, connection_id, endpoint, num_established, concurrent_dial_errors, established_in } => {
                println!("Established to {:?}", peer_id);
                let rq = RPCRequest::Ping;
                swarm.behaviour_mut().request_response.send_request(&peer_id, rq);
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(event)) => match event {
                ReqRespEvent::Message { peer, message, .. } => match message {
                    ReqRespMsg::Response { request_id, response } => {
                        println!("Received response from {:?}: {:?}", peer, response);
                    }
                    ReqRespMsg::Request { request, channel, .. } => {
                        println!("Received request from {:?}: {:?}", peer, request);
                        // send a response.. (optional!)
                        if let Err(e) = swarm.behaviour_mut().request_response.send_response(channel, RPCResponse::Pong) {
                            eprintln!("Failed to send response: {:?}", e);
                        }
                    }
                },
                ReqRespEvent::OutboundFailure { peer, error, .. } => {
                    eprintln!("Outbound failure to {:?}: {:?}", peer, error);
                }
                ReqRespEvent::InboundFailure { peer, error, .. } => {
                    eprintln!("Inbound failure from {:?}: {:?}", peer, error);
                }
                ReqRespEvent::ResponseSent { peer, .. } => {
                    println!("Response sent to {:?}", peer);
                }
            },
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on {:?}", address);
            }
            _ => {}
        }
    }


    Ok(())
}
