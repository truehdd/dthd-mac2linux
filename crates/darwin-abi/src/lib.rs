//! Darwin ABI adapters for the macOS x86-64 dee_dthd_encoder.
//!
//! FFI entry points require pointers and lengths valid for the corresponding Darwin
//! API. They are not general-purpose safe Rust interfaces.

pub mod fft;
pub mod math;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod platform;
