#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss)]
#![allow(clippy::doc_markdown, clippy::module_name_repetitions, clippy::too_many_lines)]
#![doc = include_str!("../README.md")]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod array;
mod bits;
mod decode;
mod error;
mod huffman;
mod lz;

pub use decode::{decode, decode_to_vec};
pub use error::{Error, Result};
