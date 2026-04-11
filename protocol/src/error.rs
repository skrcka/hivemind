//! Codec errors. Returned by [`encode_frame`] and [`decode_frame`].
//!
//! [`encode_frame`]: crate::codec::encode_frame
//! [`decode_frame`]: crate::codec::decode_frame

use core::fmt;

/// Errors that can occur while encoding or decoding a wire frame.
#[derive(Debug)]
pub enum CodecError {
    /// Postcard serialization or deserialization failed.
    Postcard(postcard::Error),
    /// COBS framing was invalid (corrupt or truncated frame body).
    Cobs,
    /// An empty frame body was supplied for decoding.
    EmptyFrame,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postcard(e) => write!(f, "postcard codec error: {e}"),
            Self::Cobs => write!(f, "COBS framing error"),
            Self::EmptyFrame => write!(f, "empty frame body"),
        }
    }
}

impl core::error::Error for CodecError {}

impl From<postcard::Error> for CodecError {
    fn from(e: postcard::Error) -> Self {
        Self::Postcard(e)
    }
}
