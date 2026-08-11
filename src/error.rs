//! Typed errors for BitBabbler discovery, configuration, and I/O.

use std::fmt;

/// Errors produced by [`crate::BitBabbler`] and related helpers.
///
/// Variants are intended for programmatic matching. Display text is for
/// diagnostics only and is not a stable wire protocol.
#[derive(Debug, Clone)]
pub enum BitBabblerError {
    /// No recognized BitBabbler White/Black device was found.
    NoDevice,
    /// More than one recognized device is present; select by serial.
    MultipleDevices {
        /// Number of recognized devices enumerated.
        count: usize,
    },
    /// No device matched the requested serial number.
    DeviceNotFound {
        /// Serial string that was requested.
        serial: String,
    },
    /// `open_by_serial` was called with an empty serial string.
    MissingSerial,
    /// A device with the expected VID:PID reported an unsupported product string.
    UnsupportedProduct {
        /// Product string reported by the device.
        product: String,
    },
    /// `get_bits` was called with zero bits.
    ZeroBitLength,
    /// `get_bits` was called with a bit length not divisible by 8.
    BitLengthNotByteAligned {
        /// The requested bit length that failed validation.
        requested_bits: usize,
    },
    /// A fold value outside `0..=4` was supplied.
    InvalidFold {
        /// The rejected fold value.
        value: u8,
    },
    /// `random_range` received an empty or inverted semi-open range.
    InvalidRange {
        /// Inclusive start of the requested range.
        start: u64,
        /// Exclusive end of the requested range.
        end: u64,
    },
    /// Fallible buffer reservation failed before successful completion.
    AllocationFailed {
        /// Bit length that motivated the allocation.
        requested_bits: usize,
    },
    /// The process lacks permission to open or claim the USB device.
    PermissionDenied,
    /// The device interface is already claimed by another process or handle.
    DeviceBusy,
    /// The device was removed or the handle was invalidated.
    DeviceDisconnected,
    /// A USB transfer timed out.
    TransferTimeout {
        /// Logical operation name for diagnostics.
        operation: &'static str,
    },
    /// A lower-level USB failure not mapped to a more specific variant.
    Usb {
        /// Logical operation name for diagnostics.
        operation: &'static str,
        /// Underlying `rusb` error when available.
        source: Option<rusb::Error>,
    },
    /// Framing, status, or command/response rules were violated.
    ProtocolViolation {
        /// Logical operation name for diagnostics.
        operation: &'static str,
    },
    /// FTDI/MPSSE initialization exhausted its retry budget.
    InitializationFailed {
        /// Number of full init attempts performed.
        attempts: u32,
    },
    /// Empty or incomplete entropy reads exhausted the retry budget.
    ReadRetriesExhausted {
        /// Number of empty/partial retry attempts performed.
        attempts: u32,
    },
    /// Rejection sampling for `random_range` exhausted its sample budget.
    RangeSamplingExhausted {
        /// Number of range samples attempted.
        attempts: u32,
    },
}

impl BitBabblerError {
    pub(crate) fn from_rusb(operation: &'static str, err: rusb::Error) -> Self {
        match err {
            rusb::Error::Access => Self::PermissionDenied,
            rusb::Error::Busy => Self::DeviceBusy,
            rusb::Error::NoDevice => Self::DeviceDisconnected,
            rusb::Error::Timeout => Self::TransferTimeout { operation },
            other => Self::Usb {
                operation,
                source: Some(other),
            },
        }
    }

    pub(crate) fn protocol(operation: &'static str) -> Self {
        Self::ProtocolViolation { operation }
    }
}

impl fmt::Display for BitBabblerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "no BitBabbler White or Black device found"),
            Self::MultipleDevices { count } => write!(
                f,
                "multiple BitBabbler devices found ({count}); open by serial"
            ),
            Self::DeviceNotFound { serial } => {
                write!(f, "no BitBabbler device with serial '{serial}'")
            }
            Self::MissingSerial => write!(f, "serial number must not be empty"),
            Self::UnsupportedProduct { product } => {
                write!(f, "unsupported BitBabbler product '{product}'")
            }
            Self::ZeroBitLength => write!(f, "bit length must be greater than zero"),
            Self::BitLengthNotByteAligned { requested_bits } => {
                write!(f, "bit length must be divisible by 8, got {requested_bits}")
            }
            Self::InvalidFold { value } => {
                write!(f, "fold must be between 0 and 4 inclusive, got {value}")
            }
            Self::InvalidRange { start, end } => write!(
                f,
                "invalid range: start ({start}) must be less than end ({end})"
            ),
            Self::AllocationFailed { requested_bits } => {
                write!(f, "failed to reserve buffer for {requested_bits} bit(s)")
            }
            Self::PermissionDenied => {
                write!(f, "permission denied while accessing the USB device")
            }
            Self::DeviceBusy => write!(f, "USB device interface is busy"),
            Self::DeviceDisconnected => write!(f, "USB device disconnected or handle invalid"),
            Self::TransferTimeout { operation } => {
                write!(f, "USB transfer timed out during {operation}")
            }
            Self::Usb { operation, source } => match source {
                Some(err) => write!(f, "USB error during {operation}: {err}"),
                None => write!(f, "USB error during {operation}"),
            },
            Self::ProtocolViolation { operation } => {
                write!(f, "protocol violation during {operation}")
            }
            Self::InitializationFailed { attempts } => write!(
                f,
                "FTDI/MPSSE initialization failed after {attempts} attempt(s)"
            ),
            Self::ReadRetriesExhausted { attempts } => {
                write!(
                    f,
                    "entropy read retries exhausted after {attempts} attempt(s)"
                )
            }
            Self::RangeSamplingExhausted { attempts } => write!(
                f,
                "range rejection sampling exhausted after {attempts} sample(s)"
            ),
        }
    }
}

impl std::error::Error for BitBabblerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usb {
                source: Some(err), ..
            } => Some(err),
            _ => None,
        }
    }
}

impl PartialEq for BitBabblerError {
    fn eq(&self, other: &Self) -> bool {
        use BitBabblerError::*;
        match (self, other) {
            (NoDevice, NoDevice)
            | (MissingSerial, MissingSerial)
            | (ZeroBitLength, ZeroBitLength)
            | (PermissionDenied, PermissionDenied)
            | (DeviceBusy, DeviceBusy)
            | (DeviceDisconnected, DeviceDisconnected) => true,
            (MultipleDevices { count: a }, MultipleDevices { count: b }) => a == b,
            (DeviceNotFound { serial: a }, DeviceNotFound { serial: b }) => a == b,
            (UnsupportedProduct { product: a }, UnsupportedProduct { product: b }) => a == b,
            (
                BitLengthNotByteAligned { requested_bits: a },
                BitLengthNotByteAligned { requested_bits: b },
            ) => a == b,
            (InvalidFold { value: a }, InvalidFold { value: b }) => a == b,
            (InvalidRange { start: a, end: b }, InvalidRange { start: c, end: d }) => {
                a == c && b == d
            }
            (AllocationFailed { requested_bits: a }, AllocationFailed { requested_bits: b }) => {
                a == b
            }
            (TransferTimeout { operation: a }, TransferTimeout { operation: b }) => a == b,
            (ProtocolViolation { operation: a }, ProtocolViolation { operation: b }) => a == b,
            (InitializationFailed { attempts: a }, InitializationFailed { attempts: b }) => a == b,
            (ReadRetriesExhausted { attempts: a }, ReadRetriesExhausted { attempts: b }) => a == b,
            (RangeSamplingExhausted { attempts: a }, RangeSamplingExhausted { attempts: b }) => {
                a == b
            }
            (
                Usb {
                    operation: a,
                    source: sa,
                },
                Usb {
                    operation: b,
                    source: sb,
                },
            ) => a == b && sa == sb,
            _ => false,
        }
    }
}

impl Eq for BitBabblerError {}

#[cfg(test)]
mod tests {
    use super::BitBabblerError;

    #[test]
    fn display_messages_are_non_empty() {
        let cases = [
            BitBabblerError::NoDevice,
            BitBabblerError::MultipleDevices { count: 2 },
            BitBabblerError::DeviceNotFound {
                serial: "ABC".into(),
            },
            BitBabblerError::MissingSerial,
            BitBabblerError::UnsupportedProduct {
                product: "Other".into(),
            },
            BitBabblerError::ZeroBitLength,
            BitBabblerError::BitLengthNotByteAligned { requested_bits: 7 },
            BitBabblerError::InvalidFold { value: 5 },
            BitBabblerError::InvalidRange { start: 5, end: 2 },
            BitBabblerError::AllocationFailed { requested_bits: 64 },
            BitBabblerError::PermissionDenied,
            BitBabblerError::DeviceBusy,
            BitBabblerError::DeviceDisconnected,
            BitBabblerError::TransferTimeout {
                operation: "bulk_read",
            },
            BitBabblerError::Usb {
                operation: "open",
                source: None,
            },
            BitBabblerError::ProtocolViolation { operation: "sync" },
            BitBabblerError::InitializationFailed { attempts: 20 },
            BitBabblerError::ReadRetriesExhausted { attempts: 10 },
            BitBabblerError::RangeSamplingExhausted { attempts: 100 },
        ];
        for err in cases {
            assert!(!err.to_string().is_empty(), "{err:?}");
        }
    }

    #[test]
    fn variants_are_distinguishable() {
        assert_ne!(BitBabblerError::NoDevice, BitBabblerError::DeviceBusy);
        assert_ne!(
            BitBabblerError::MultipleDevices { count: 2 },
            BitBabblerError::MultipleDevices { count: 3 }
        );
    }
}
