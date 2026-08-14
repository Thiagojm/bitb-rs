//! Public error Display and distinguishability.

use bitb_rs::{BitBabblerError, ProtocolOperation, UsbOperation};

#[test]
fn display_non_empty_for_public_variants() {
    let cases = [
        BitBabblerError::NoDevice,
        BitBabblerError::MultipleDevices { count: 2 },
        BitBabblerError::DeviceNotFound { serial: "X".into() },
        BitBabblerError::MissingSerial,
        BitBabblerError::UnsupportedProduct {
            product: "Other".into(),
        },
        BitBabblerError::ZeroBitLength,
        BitBabblerError::BitLengthNotByteAligned { requested_bits: 3 },
        BitBabblerError::InvalidFold { value: 9 },
        BitBabblerError::InvalidRange { start: 1, end: 1 },
        BitBabblerError::AllocationFailed { requested_bits: 8 },
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
        BitBabblerError::InitializationFailed {
            attempts: 1,
            source: Box::new(BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::MpsseSync,
            }),
        },
        BitBabblerError::ReadRetriesExhausted { attempts: 1 },
        BitBabblerError::RangeSamplingExhausted { attempts: 1 },
    ];
    for err in cases {
        assert!(!err.to_string().is_empty(), "{err:?}");
    }
}

#[test]
fn matchable_variants() {
    let err = BitBabblerError::MultipleDevices { count: 3 };
    match err {
        BitBabblerError::MultipleDevices { count } => assert_eq!(count, 3),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn initialization_failed_exposes_source() {
    let err = BitBabblerError::InitializationFailed {
        attempts: 2,
        source: Box::new(BitBabblerError::ProtocolViolation {
            operation: ProtocolOperation::ModemStatus,
        }),
    };
    assert_eq!(
        std::error::Error::source(&err)
            .map(ToString::to_string)
            .as_deref(),
        Some("protocol violation during modem_status")
    );
}
