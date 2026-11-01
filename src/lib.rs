//! Build dee_dthd_encoder for Linux from its macOS x86-64 payload.

use sha2::{Digest, Sha256};

pub mod build;
pub mod fetch;
pub mod generate;
pub mod image;
pub mod textmap;
pub mod version;

pub fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
