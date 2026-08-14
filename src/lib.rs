#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod device;
mod error;
mod fold;
mod policy;
mod protocol;
mod transport;

pub use device::{BitBabbler, DeviceInfo, DeviceVariant};
pub use error::{BitBabblerError, ProtocolOperation, UsbOperation};
pub use fold::Fold;
