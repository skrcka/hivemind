//! Wire codec — postcard serialization + COBS framing.
//!
//! Every frame on the wire is `cobs(postcard(envelope)) || 0x00`.
//!
//! - **Postcard** (compact serde-driven binary) gives us small messages
//!   without writing a hand-rolled wire format.
//! - **COBS** (Consistent Overhead Byte Stuffing) gives us self-synchronising
//!   frame boundaries on a serial byte stream — the trailing `0x00` is the
//!   delimiter and is guaranteed to never appear inside a frame.
//!
//! The same codec is used for serial radios *and* TCP — only the byte
//! source/sink differs, the bytes are identical.

use alloc::vec;
use alloc::vec::Vec;
use serde::{de::DeserializeOwned, Serialize};

use crate::error::CodecError;
use crate::messages::Envelope;

/// COBS encoding adds at most 1 overhead byte per 254 bytes of input
/// (`(n / 254) + 1`), plus we append a trailing `0x00` delimiter — so worst
/// case the encoded frame is `n + (n / 254) + 2` bytes.
const fn cobs_max_encoded_len(input_len: usize) -> usize {
    input_len + (input_len / 254) + 1
}

/// Encode an envelope into a complete on-the-wire frame:
/// `cobs(postcard(envelope)) || 0x00`.
///
/// The returned `Vec` always ends with the `0x00` delimiter, so it can be
/// written directly to the wire.
pub fn encode_frame<T: Serialize>(envelope: &Envelope<T>) -> Result<Vec<u8>, CodecError> {
    let postcard_bytes = postcard::to_allocvec(envelope)?;

    let max_size = cobs_max_encoded_len(postcard_bytes.len());
    let mut frame = vec![0u8; max_size + 1]; // +1 for the trailing delimiter
    let encoded_len = cobs::encode(&postcard_bytes, &mut frame[..max_size]);
    frame[encoded_len] = 0x00;
    frame.truncate(encoded_len + 1);
    Ok(frame)
}

/// Decode a single COBS-encoded frame body (without the trailing `0x00`) into
/// a typed envelope.
///
/// The input is the bytes between two `0x00` delimiters, as produced by
/// [`FrameDecoder::push`] / [`FrameDecoder::push_slice`].
pub fn decode_frame<T: DeserializeOwned>(cobs_body: &[u8]) -> Result<Envelope<T>, CodecError> {
    if cobs_body.is_empty() {
        return Err(CodecError::EmptyFrame);
    }
    // COBS decoding produces at most as many bytes as the input.
    let mut decoded = vec![0u8; cobs_body.len()];
    let decoded_len = cobs::decode(cobs_body, &mut decoded).map_err(|_| CodecError::Cobs)?;
    decoded.truncate(decoded_len);
    let env = postcard::from_bytes(&decoded)?;
    Ok(env)
}

/// Streaming frame decoder. Push bytes from the wire as they arrive (one byte
/// at a time, or in arbitrary chunks); the decoder buffers them until a
/// `0x00` delimiter is seen, at which point it produces a complete COBS-
/// encoded frame body.
///
/// Pass each produced body to [`decode_frame`] to get a typed envelope.
///
/// The decoder is robust to byte loss: if a frame is corrupt, it will be
/// dropped at the next delimiter and decoding resumes from the byte after.
///
/// # Example
///
/// ```
/// use hivemind_protocol::{decode_frame, encode_frame, Envelope, FrameDecoder, OracleToLegion};
///
/// let env = Envelope::new("drone-01", 0, OracleToLegion::Heartbeat);
/// let frame_bytes = encode_frame(&env).unwrap();
///
/// let mut decoder = FrameDecoder::new();
/// // Feed the bytes one at a time, just to prove the streaming case works:
/// let mut decoded = None;
/// for &b in &frame_bytes {
///     if let Some(body) = decoder.push(b) {
///         decoded = Some(decode_frame::<OracleToLegion>(&body).unwrap());
///     }
/// }
/// assert_eq!(decoded.unwrap(), env);
/// ```
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// Create a new decoder with no preallocated buffer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Create a new decoder with the given initial capacity for the
    /// in-progress frame buffer.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Push a single byte. Returns `Some(body)` if a complete COBS-encoded
    /// frame body is now ready (the trailing `0x00` is consumed and not
    /// included in the body). Returns `None` if more bytes are needed.
    pub fn push(&mut self, byte: u8) -> Option<Vec<u8>> {
        if byte == 0 {
            if self.buf.is_empty() {
                // Empty frame between delimiters; ignore.
                return None;
            }
            Some(core::mem::take(&mut self.buf))
        } else {
            self.buf.push(byte);
            None
        }
    }

    /// Push a slice of bytes. Returns every complete frame produced (zero,
    /// one, or many).
    pub fn push_slice(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for &b in bytes {
            if let Some(frame) = self.push(b) {
                frames.push(frame);
            }
        }
        frames
    }

    /// Number of bytes currently buffered (i.e. waiting for a delimiter).
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Clear any partially-decoded state. Use this after a known
    /// synchronization loss.
    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}
