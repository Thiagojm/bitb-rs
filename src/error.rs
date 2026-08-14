//! Typed errors for BitBabbler discovery, configuration, and I/O.

use std::fmt;

/// USB transfer or descriptor operation named by a typed error.
///
/// Used by [`BitBabblerError::TransferTimeout`] and [`BitBabblerError::Usb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UsbOperation {
    /// Open a USB device handle.
    Open,
    /// Enumerate USB devices.
    ListDevices,
    /// Read the device descriptor.
    DeviceDescriptor,
    /// Read a configuration descriptor.
    ConfigDescriptor,
    /// Set the active USB configuration.
    SetConfiguration,
    /// Claim the FTDI interface.
    ClaimInterface,
    /// Release the FTDI interface.
    ReleaseInterface,
    /// Vendor control transfer (host to device).
    ControlOut,
    /// Vendor control transfer (device to host).
    ControlIn,
    /// Bulk OUT transfer.
    BulkWrite,
    /// Bulk IN transfer.
    BulkRead,
    /// Read the USB product string descriptor.
    ReadProductString,
    /// Read the USB serial string descriptor.
    ReadSerialString,
}

impl UsbOperation {
    /// Stable diagnostic label for this operation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ListDevices => "list_devices",
            Self::DeviceDescriptor => "device_descriptor",
            Self::ConfigDescriptor => "config_descriptor",
            Self::SetConfiguration => "set_configuration",
            Self::ClaimInterface => "claim_interface",
            Self::ReleaseInterface => "release_interface",
            Self::ControlOut => "control_out",
            Self::ControlIn => "control_in",
            Self::BulkWrite => "bulk_write",
            Self::BulkRead => "bulk_read",
            Self::ReadProductString => "read_product_string",
            Self::ReadSerialString => "read_serial_string",
        }
    }
}

impl fmt::Display for UsbOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Protocol, descriptor-policy, or framing failure named by a typed error.
///
/// Used by [`BitBabblerError::ProtocolViolation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProtocolOperation {
    /// Live endpoint layout no longer matches the enumerated device.
    EndpointConfigChanged,
    /// Device descriptor omitted the product string index.
    MissingProductStringIndex,
    /// Device descriptor omitted the serial string index.
    MissingSerialStringIndex,
    /// Product string was empty after trimming.
    EmptyProductString,
    /// Serial string was empty after trimming.
    EmptySerialString,
    /// Active configuration number is not the required value.
    UsbConfiguration,
    /// Required interface / alternate setting was not found.
    UsbInterface,
    /// Interface does not expose exactly two endpoints.
    EndpointCount,
    /// First endpoint is not IN.
    EndpointInDirection,
    /// Second endpoint is not OUT.
    EndpointOutDirection,
    /// An endpoint is not bulk.
    EndpointTransferType,
    /// IN max-packet size is too small to carry FTDI status.
    MaxPacketSize,
    /// IN and OUT max-packet sizes differ.
    MaxPacketMismatch,
    /// Max-packet size is not 64 or 512.
    UnsupportedMaxPacket,
    /// MPSSE AA/AB sync handshake failed.
    MpsseSync,
    /// Requested MPSSE read length is zero or above the command limit.
    MpsseReadLength,
    /// Device returned more payload than the MPSSE command requested.
    ExcessPayload,
    /// Final line status was not exactly THRE|TEMT.
    IncompleteLineStatus,
    /// FTDI modem status byte did not match the expected packet-size marker.
    ModemStatus,
    /// FTDI line status contained illegal bits.
    LineStatus,
    /// Bulk OUT transferred zero bytes.
    BulkWriteZero,
    /// GET_MODEM_STATUS did not return two bytes.
    GetModemStatusLen,
}

impl ProtocolOperation {
    /// Stable diagnostic label for this operation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndpointConfigChanged => "endpoint_config_changed",
            Self::MissingProductStringIndex => "missing_product_string_index",
            Self::MissingSerialStringIndex => "missing_serial_string_index",
            Self::EmptyProductString => "empty_product_string",
            Self::EmptySerialString => "empty_serial_string",
            Self::UsbConfiguration => "usb_configuration",
            Self::UsbInterface => "usb_interface",
            Self::EndpointCount => "endpoint_count",
            Self::EndpointInDirection => "endpoint_in_direction",
            Self::EndpointOutDirection => "endpoint_out_direction",
            Self::EndpointTransferType => "endpoint_transfer_type",
            Self::MaxPacketSize => "max_packet_size",
            Self::MaxPacketMismatch => "max_packet_mismatch",
            Self::UnsupportedMaxPacket => "unsupported_max_packet",
            Self::MpsseSync => "mpsse_sync",
            Self::MpsseReadLength => "mpsse_read_length",
            Self::ExcessPayload => "excess_payload",
            Self::IncompleteLineStatus => "incomplete_line_status",
            Self::ModemStatus => "modem_status",
            Self::LineStatus => "line_status",
            Self::BulkWriteZero => "bulk_write_zero",
            Self::GetModemStatusLen => "get_modem_status_len",
        }
    }
}

impl fmt::Display for ProtocolOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors produced by [`crate::BitBabbler`] and related helpers.
///
/// Variants are intended for programmatic matching. Display text is for
/// diagnostics only and is not a stable wire protocol.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
        /// USB operation that timed out.
        operation: UsbOperation,
    },
    /// A lower-level USB failure not mapped to a more specific variant.
    Usb {
        /// USB operation that failed.
        operation: UsbOperation,
        /// Underlying `rusb` error when available.
        source: Option<rusb::Error>,
    },
    /// Framing, status, or command/response rules were violated.
    ProtocolViolation {
        /// Protocol check that failed.
        operation: ProtocolOperation,
    },
    /// FTDI/MPSSE initialization exhausted its retry budget.
    InitializationFailed {
        /// Number of full init attempts performed.
        attempts: u32,
        /// Last non-fatal error observed before giving up.
        source: Box<BitBabblerError>,
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
    pub(crate) fn from_rusb(operation: UsbOperation, err: rusb::Error) -> Self {
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

    pub(crate) fn protocol(operation: ProtocolOperation) -> Self {
        Self::ProtocolViolation { operation }
    }

    pub(crate) fn initialization_failed(attempts: u32, source: Self) -> Self {
        Self::InitializationFailed {
            attempts,
            source: Box::new(source),
        }
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
            Self::InitializationFailed { attempts, source } => write!(
                f,
                "FTDI/MPSSE initialization failed after {attempts} attempt(s): {source}"
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
            Self::InitializationFailed { source, .. } => Some(source.as_ref()),
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
            (
                InitializationFailed {
                    attempts: a,
                    source: sa,
                },
                InitializationFailed {
                    attempts: b,
                    source: sb,
                },
            ) => a == b && sa == sb,
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
    use super::{BitBabblerError, ProtocolOperation, UsbOperation};

    fn sample_init_failed() -> BitBabblerError {
        BitBabblerError::initialization_failed(
            20,
            BitBabblerError::protocol(ProtocolOperation::MpsseSync),
        )
    }

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
                operation: UsbOperation::BulkRead,
            },
            BitBabblerError::Usb {
                operation: UsbOperation::Open,
                source: None,
            },
            BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::MpsseSync,
            },
            sample_init_failed(),
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

    #[test]
    fn initialization_failed_preserves_source() {
        let err = sample_init_failed();
        assert_eq!(
            std::error::Error::source(&err)
                .map(ToString::to_string)
                .as_deref(),
            Some("protocol violation during mpsse_sync")
        );
        assert!(
            err.to_string()
                .contains("protocol violation during mpsse_sync")
        );
    }
}
