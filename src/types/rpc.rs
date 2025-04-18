// src/mpc_request_response.rs

use libp2p::{
    request_response::{
        cbor::Behaviour as CborReqRespBehaviour, Codec as  RequestResponseCodec, Config as RequestResponseConfig, ProtocolSupport
    }, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use futures::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use std::{io, pin::Pin, future::Future};
use super::PeerInfo;

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
    Ping,
    GetPeerInfo,
    Custom { id: String, data: Vec<u8> },
}

/// RPC response variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RPCResponse {
    Pong,
    PeerInfo(PeerInfo),
    Custom { id: String, data: Vec<u8> },
    Error(String),
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
