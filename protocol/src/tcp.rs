//! TCP transport. Used for SITL, dev, and any future IP-based radio.
//! Behind the `tcp` feature flag.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::{de::DeserializeOwned, Serialize};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::codec::{decode_frame, encode_frame, FrameDecoder};
use crate::error::CodecError;
use crate::messages::Envelope;
use crate::transport::Transport;

/// Errors returned by [`TcpTransport`].
#[derive(Debug)]
pub enum TcpTransportError {
    Codec(CodecError),
    Io(io::Error),
    /// The peer cleanly closed the connection (read returned 0 bytes).
    Closed,
}

impl fmt::Display for TcpTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(e) => write!(f, "codec error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Closed => write!(f, "transport closed by peer"),
        }
    }
}

impl std::error::Error for TcpTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Closed => None,
        }
    }
}

impl From<CodecError> for TcpTransportError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}

impl From<io::Error> for TcpTransportError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// `Transport` impl over a [`tokio::net::TcpStream`].
///
/// Owns the stream, a streaming COBS decoder, a read buffer, and a queue of
/// pending decoded frames so that a single TCP read which produces multiple
/// frames doesn't drop any.
pub struct TcpTransport<TX, RX> {
    stream: TcpStream,
    decoder: FrameDecoder,
    read_buf: Vec<u8>,
    pending: VecDeque<Vec<u8>>,
    _phantom: PhantomData<fn(TX) -> RX>,
}

impl<TX, RX> TcpTransport<TX, RX> {
    /// Wrap an existing connected `TcpStream`.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            decoder: FrameDecoder::with_capacity(4096),
            read_buf: vec![0u8; 4096],
            pending: VecDeque::new(),
            _phantom: PhantomData,
        }
    }

    /// Consume the transport and return the underlying `TcpStream`. Useful
    /// when you want to upgrade to TLS or hand the socket to another
    /// component.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

impl<TX, RX> Transport<TX, RX> for TcpTransport<TX, RX>
where
    TX: Serialize + Send + Sync,
    RX: DeserializeOwned + Send + Sync,
{
    type Error = TcpTransportError;

    async fn send(&mut self, msg: &Envelope<TX>) -> Result<(), Self::Error> {
        let frame = encode_frame(msg)?;
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Envelope<RX>, Self::Error> {
        loop {
            if let Some(body) = self.pending.pop_front() {
                return decode_frame(&body).map_err(Into::into);
            }
            let n = self.stream.read(&mut self.read_buf).await?;
            if n == 0 {
                return Err(TcpTransportError::Closed);
            }
            for frame in self.decoder.push_slice(&self.read_buf[..n]) {
                self.pending.push_back(frame);
            }
        }
    }
}
