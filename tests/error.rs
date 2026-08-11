//! Public error Display and distinguishability.

use bitb_rs::BitBabblerError;

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
            operation: "bulk_read",
        },
        BitBabblerError::Usb {
            operation: "open",
            source: None,
        },
        BitBabblerError::ProtocolViolation { operation: "sync" },
        BitBabblerError::InitializationFailed { attempts: 1 },
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
