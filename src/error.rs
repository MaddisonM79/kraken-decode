//! The error type.

use core::fmt;

/// Why a stream could not be decoded.
///
/// Every failure is reported through this type: the decoder does not panic on
/// any input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The input ended before the structure it describes was complete.
    Truncated,
    /// The input does not follow the format: a header field, a table or a
    /// command contradicts the rest of the stream. The string names the part.
    Corrupt(&'static str),
    /// The input uses a part of the format this crate does not decode. The
    /// string names the part. See the README for what is covered.
    Unsupported(&'static str),
    /// The stream decoded to fewer or more bytes than the output buffer holds,
    /// or bytes were left over after the output was full.
    SizeMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => f.write_str("compressed data is truncated"),
            Error::Corrupt(what) => write!(f, "compressed data is corrupt: {what}"),
            Error::Unsupported(what) => write!(f, "unsupported stream feature: {what}"),
            Error::SizeMismatch => f.write_str("compressed data does not match the expected decoded size"),
        }
    }
}

impl core::error::Error for Error {}

/// Shorthand for `Result<T, Error>`.
pub type Result<T> = core::result::Result<T, Error>;
