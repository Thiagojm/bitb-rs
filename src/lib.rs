//! # bitb-rs
//!
//! Synchronous access to a single TNRG **BitBabbler** White or Black hardware
//! RNG over USB (Windows and Linux).
//!
//! ## Guarantees
//!
//! - **One device per handle.** Each [`BitBabbler`] owns a single USB interface.
//! - **Raw by default.** [`BitBabbler::get_bits`] returns device bytes with only
//!   FTDI status framing removed. Folding is opt-in via
//!   [`BitBabbler::get_bits_with_fold`].
//! - **All or nothing.** Collection methods return a complete buffer or an error;
//!   partial data is never exposed.
//! - **No health checks.** This crate does not run ENT, FIPS, or any statistical
//!   gating. Quality evaluation belongs in the consumer.
//! - **No automatic reconnection.** After disconnect or an invalidating reset,
//!   open a new handle.
//!
//! ## Platform requirements
//!
//! | Platform | Requirement |
//! |----------|-------------|
//! | Windows  | Device bound to **WinUSB** (manual association; this crate does not install drivers) |
//! | Linux    | Permission to open `0403:7840` (typically a restricted udev rule) |
//!
//! ## Example
//!
//! ```no_run
//! use bitb_rs::{BitBabbler, BitBabblerError, Fold};
//!
//! fn main() -> Result<(), BitBabblerError> {
//!     let mut dev = BitBabbler::open()?;
//!     let raw = dev.get_bits(256)?;
//!     assert_eq!(raw.len(), 32);
//!
//!     let folded = dev.get_bits_with_fold(256, Fold::One)?;
//!     assert_eq!(folded.len(), 32);
//!
//!     let word = dev.random_u64()?;
//!     let bounded = dev.random_range(10..20)?;
//!     assert!((10..20).contains(&bounded));
//!     let _ = word;
//!     Ok(())
//! }
//! ```
//!
//! ## Async hosts
//!
//! The crate is intentionally synchronous. Hosts using Tokio/Tauri should move
//! blocking calls to `spawn_blocking` (or equivalent).

#![warn(missing_docs)]

mod device;
mod error;
mod fold;
mod policy;
mod protocol;
mod transport;

pub use device::{BitBabbler, DeviceInfo, DeviceVariant};
pub use error::BitBabblerError;
pub use fold::Fold;
