//! [`Transport`] trait — abstract bidirectional message transport.
//!
//! The trait is generic over the *direction*: `TX` is what this side sends,
//! `RX` is what it expects to receive. Oracle uses
//! `Transport<OracleToLegion, LegionToOracle>`; legion uses the dual
//! `Transport<LegionToOracle, OracleToLegion>`.

use core::fmt::Debug;
use core::future::Future;
use serde::{de::DeserializeOwned, Serialize};

use crate::messages::Envelope;

/// A bidirectional message transport with typed send and receive directions.
///
/// Concrete implementations carry COBS-postcard frames over a byte source/
/// sink — `TcpStream`, `tokio_serial::SerialStream`, or (for the future MCU
/// port) `embassy-stm32-usart`. The trait itself is runtime-agnostic; it uses
/// stable `async fn in trait` (Rust ≥1.75) so any async runtime can host the
/// implementation.
///
/// # Type parameters
///
/// - `TX` — the message type this side *sends*. Must be `Serialize`.
/// - `RX` — the message type this side expects to *receive*. Must be
///   `DeserializeOwned`.
///
/// # Example
///
/// Oracle's per-drone session task would type the transport as:
///
/// ```ignore
/// use hivemind_protocol::{LegionToOracle, OracleToLegion, Transport};
/// async fn session<T: Transport<OracleToLegion, LegionToOracle>>(mut t: T) {
///     // ...
/// }
/// ```
pub trait Transport<TX, RX>: Send
where
    TX: Serialize + Send + Sync,
    RX: DeserializeOwned + Send + Sync,
{
    type Error: Debug + Send;

    /// Send a single envelope. Blocks until the frame is fully written and
    /// flushed to the underlying byte sink.
    fn send(
        &mut self,
        msg: &Envelope<TX>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Receive a single envelope. Blocks until a complete frame is read and
    /// decoded.
    fn recv(&mut self) -> impl Future<Output = Result<Envelope<RX>, Self::Error>> + Send;
}
