//! Serial transport. Used for production EU SiK-class radios (HolyBro 433
//! MHz at v1, RFD868x at v2). Behind the `serial` feature flag.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use serde::{de::DeserializeOwned, Serialize};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

use crate::codec::{decode_frame, encode_frame, FrameDecoder};
use crate::error::CodecError;
use crate::messages::Envelope;
use crate::transport::Transport;

/// Errors returned by [`SerialTransport`].
#[derive(Debug)]
pub enum SerialTransportError {
    Codec(CodecError),
    Io(io::Error),
    Serial(tokio_serial::Error),
    /// The serial port returned EOF (read returned 0 bytes). For a real
    /// serial port this typically means the device was unplugged.
    Closed,
}

impl fmt::Display for SerialTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(e) => write!(f, "codec error: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Serial(e) => write!(f, "serial port error: {e}"),
            Self::Closed => write!(f, "serial port closed (EOF)"),
        }
    }
}

impl std::error::Error for SerialTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Serial(e) => Some(e),
            Self::Closed => None,
        }
    }
}

impl From<CodecError> for SerialTransportError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}

impl From<io::Error> for SerialTransportError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<tokio_serial::Error> for SerialTransportError {
    fn from(e: tokio_serial::Error) -> Self {
        Self::Serial(e)
    }
}

/// `Transport` impl over a [`tokio_serial::SerialStream`].
///
/// Owns the port, a streaming COBS decoder, a read buffer, and a queue of
/// pending decoded frames.
pub struct SerialTransport<TX, RX> {
    port: SerialStream,
    decoder: FrameDecoder,
    read_buf: Vec<u8>,
    pending: VecDeque<Vec<u8>>,
    _phantom: PhantomData<fn(TX) -> RX>,
}

impl<TX, RX> SerialTransport<TX, RX> {
    /// Open a serial port at the given path and baud rate.
    pub fn open(path: &str, baud: u32) -> Result<Self, SerialTransportError> {
        let port = tokio_serial::new(path, baud).open_native_async()?;
        Ok(Self::from_port(port))
    }

    /// Wrap an existing opened `SerialStream`.
    pub fn from_port(port: SerialStream) -> Self {
        Self {
            port,
            decoder: FrameDecoder::with_capacity(4096),
            read_buf: vec![0u8; 1024],
            pending: VecDeque::new(),
            _phantom: PhantomData,
        }
    }
}

impl<TX, RX> Transport<TX, RX> for SerialTransport<TX, RX>
where
    TX: Serialize + Send + Sync,
    RX: DeserializeOwned + Send + Sync,
{
    type Error = SerialTransportError;

    async fn send(&mut self, msg: &Envelope<TX>) -> Result<(), Self::Error> {
        let frame = encode_frame(msg)?;
        self.port.write_all(&frame).await?;
        self.port.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Envelope<RX>, Self::Error> {
        loop {
            if let Some(body) = self.pending.pop_front() {
                return decode_frame(&body).map_err(Into::into);
            }
            let n = self.port.read(&mut self.read_buf).await?;
            if n == 0 {
                return Err(SerialTransportError::Closed);
            }
            for frame in self.decoder.push_slice(&self.read_buf[..n]) {
                self.pending.push_back(frame);
            }
        }
    }
}
