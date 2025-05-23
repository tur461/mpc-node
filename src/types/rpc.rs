// src/mpc_request_response.rs

use libp2p::{
    request_response::{
        cbor::Behaviour as CborReqRespBehaviour, Codec as  RequestResponseCodec, Config as RequestResponseConfig, ProtocolSupport
    }, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use futures::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tracing::info;
use std::{io, pin::Pin, future::Future};
use crate::dkg::DKGStatus;

use super::{p2p::SerializablePeerId, PeerInfo, SerializableG1Affine, SerializableScalar};

pub type MPCCodecRequest = <MPCCodec as libp2p::request_response::Codec>::Request;
pub type MPCCodecResponse = <MPCCodec as libp2p::request_response::Codec>::Response;

/// The protocol identifier; must match in `ProtocolSupport`.
#[derive(Debug, Clone)]
pub struct MPCProtocol;

impl AsRef<str> for MPCProtocol {
    fn as_ref(&self) -> &str {
        "/mpc/1.0.0"
    }
}

/// RPC request variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RPCRequest {
    Custom { id: String, data: Vec<u8> },
    GetDKGStatus,
    GetPeerInfo,
    Ping,
    StartDKG,

    StartADKG, 
    // { 
    //     n: usize,
    //     threshold: usize, 
    //     // participants: Vec<SerializablePeerId> 
    // },

    SendAVSSShare {
        dealer_id: SerializablePeerId,
        receiver_id: SerializablePeerId,
        share: SerializableScalar,
        commitments: Vec<SerializableG1Affine>,
    },

    BroadcastCommitments {
        dealer_id: SerializablePeerId,
        commitments: Vec<SerializableG1Affine>,
    },

    SubmitComplaint {
        dealer_id: SerializablePeerId,
        from: SerializablePeerId,
        reason: String, // e.g. "invalid share"
    },

    DeclareADKGComplete {
        from: SerializablePeerId,
        share: SerializableScalar,
    },
}

/// RPC response variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RPCResponse {
    Custom { id: String, data: Vec<u8> },
    DKGStarted {peer_id: SerializablePeerId},
    DKGError(String),
    DKGStatus {status: DKGStatus},
    Error(String),
    PeerInfo(PeerInfo),
    Pong,
    NoResponse,
    Ack,
    Verified,
    ShareAccepted,
    ComplaintAccepted,
    FinishedADKG {
        group_public_key: SerializableG1Affine,
    },
    GotAVSSShare {
        dealer_id: SerializablePeerId,
        receiver_id: SerializablePeerId,
        share: SerializableScalar,
        commitments: Vec<SerializableG1Affine>,
    }
}

/// CBOR + serde codec for our MPC protocol.
#[derive(Clone, Deserialize, Serialize)]
pub struct MPCCodec;

impl RequestResponseCodec for MPCCodec {
    type Protocol = MPCProtocol;
    type Request = RPCRequest;
    type Response = RPCResponse;

    // NOTE: These method signatures *must* match the trait exactly.
    fn read_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 MPCProtocol,
        io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<RPCRequest>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        T: AsyncRead + Unpin + Send + 'async_trait,  // nonblocking read :contentReference[oaicite:4]{index=4}
        Self: 'async_trait,
    {
        info!("[MPC REQ-RESP] Read req");
        Box::pin(async move {
            let mut buf = Vec::new();
            io.read_to_end(&mut buf).await?;      // collect the full request :contentReference[oaicite:5]{index=5}
            serde_cbor::from_slice(&buf)          // decode CBOR
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
    }

    fn read_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 MPCProtocol,
        io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<RPCResponse>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        T: AsyncRead + Unpin + Send + 'async_trait,  // nonblocking read :contentReference[oaicite:6]{index=6}
        Self: 'async_trait,
    {
        info!("[MPC REQ-RESP] Read resp");
        Box::pin(async move {
            let mut buf = Vec::new();
            io.read_to_end(&mut buf).await?;
            serde_cbor::from_slice(&buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
    }

    fn write_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 MPCProtocol,
        io: &'life2 mut T,
        req: RPCRequest,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        T: AsyncWrite + Unpin + Send + 'async_trait, // nonblocking write :contentReference[oaicite:7]{index=7}
        Self: 'async_trait,
    {
        info!("[MPC REQ-RESP] Write req");
        Box::pin(async move {
            let data = serde_cbor::to_vec(&req)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            io.write_all(&data).await?;             // send bytes
            io.flush().await                       // flush substream
        })
    }
    fn write_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 MPCProtocol,
        io: &'life2 mut T,
        res: RPCResponse,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        T: AsyncWrite + Unpin + Send + 'async_trait, // nonblocking write :contentReference[oaicite:8]{index=8}
        Self: 'async_trait,
    {
        info!("[MPC REQ-RESP] Write resp");
        Box::pin(async move {
            let data = serde_cbor::to_vec(&res)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            io.write_all(&data).await?;
            io.flush().await
        })
    }
}

/// Build a libp2p request‑response behaviour using our MPCCodec + CBOR.
///
/// You can then add this to your Swarm.
/// 
pub fn build_mpc_behaviour() -> CborReqRespBehaviour<MPCCodecRequest, MPCCodecResponse> {
    let cfg = RequestResponseConfig::default();
    let proto = StreamProtocol::new(MPCProtocol.as_ref());
    let protocols = std::iter::once((proto, ProtocolSupport::Full));
    CborReqRespBehaviour::new(protocols, cfg)
}
