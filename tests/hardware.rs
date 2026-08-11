//! Optional physical hardware integration tests.
//!
//! These tests are `#[ignore]` so the default deterministic suite never claims
//! the USB device. Run them only when a single BitBabbler is connected and
//! permissioned, and always serially:
//!
//! ```text
//! cargo test --test hardware -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! - [`BitBabblerError::NoDevice`] is treated as absence (skip with a clear message).
//! - Any other error fails the test (permission, busy, protocol, USB, etc.).
//! - [`BitBabblerError::MultipleDevices`] fails: these tests require exactly one device.
//!
//! Tests never assert statistical quality or that two samples differ.

use bitb_rs::{BitBabbler, BitBabblerError, DeviceVariant, Fold};

/// Opens the single attached device, or signals absence.
///
/// Returns `Ok(None)` only for [`BitBabblerError::NoDevice`]. Every other error
/// is returned as `Err` so the test fails.
fn try_open_required() -> Result<Option<BitBabbler>, BitBabblerError> {
    match BitBabbler::open() {
        Ok(dev) => Ok(Some(dev)),
        Err(BitBabblerError::NoDevice) => Ok(None),
        Err(e) => Err(e),
    }
}

#[test]
#[ignore = "requires a single physical BitBabbler; run with --ignored --test-threads=1"]
fn hardware_open_and_raw_read() {
    let mut dev = match try_open_required() {
        Ok(Some(d)) => d,
        Ok(None) => {
            eprintln!("hardware: SKIP hardware_open_and_raw_read — no BitBabbler device present");
            return;
        }
        Err(e) => panic!("hardware open failed (not a skip): {e}"),
    };

    let info = dev.device_info().clone();
    eprintln!(
        "hardware: opened {:?} product={}",
        info.variant, info.product
    );
    assert!(matches!(
        info.variant,
        DeviceVariant::White | DeviceVariant::Black
    ));
    assert!(
        !info.serial.is_empty(),
        "serial descriptor must be non-empty"
    );

    let raw = dev.get_bits(128).expect("raw get_bits");
    assert_eq!(raw.len(), 16);

    let word = dev.random_u64().expect("random_u64");
    let bounded = dev.random_range(0..1000).expect("random_range");
    assert!(bounded < 1000);
    let _ = word;
    eprintln!("hardware: raw + random_u64/random_range ok");
}

#[test]
#[ignore = "requires a single physical BitBabbler; run with --ignored --test-threads=1"]
fn hardware_folds_one_through_four() {
    let mut dev = match try_open_required() {
        Ok(Some(d)) => d,
        Ok(None) => {
            eprintln!(
                "hardware: SKIP hardware_folds_one_through_four — no BitBabbler device present"
            );
            return;
        }
        Err(e) => panic!("hardware open failed (not a skip): {e}"),
    };

    for fold in [Fold::One, Fold::Two, Fold::Three, Fold::Four] {
        let out = dev
            .get_bits_with_fold(64, fold)
            .unwrap_or_else(|e| panic!("fold {fold:?}: {e}"));
        assert_eq!(out.len(), 8, "fold {fold:?}");
        eprintln!("hardware: fold {fold:?} ok (8 bytes)");
    }
}

#[test]
#[ignore = "requires a single physical BitBabbler; run with --ignored --test-threads=1"]
fn hardware_list_devices_smoke() {
    let list = match BitBabbler::list_devices() {
        Ok(list) => list,
        Err(BitBabblerError::NoDevice) => {
            // list_devices returns Ok(vec![]) when none are present, not NoDevice.
            // Keep this arm only for completeness if mapping ever changes.
            eprintln!("hardware: SKIP list_devices — no BitBabbler device present");
            return;
        }
        Err(e) => panic!("hardware list_devices failed (not a skip): {e}"),
    };

    if list.is_empty() {
        eprintln!("hardware: SKIP list_devices — no BitBabbler device present");
        return;
    }

    eprintln!("hardware: list_devices returned {} entry(ies)", list.len());
    for info in &list {
        eprintln!(
            "  {:?} product={} bus={} addr={}",
            info.variant, info.product, info.bus_number, info.device_address
        );
        assert!(!info.serial.is_empty());
        assert!(matches!(
            info.variant,
            DeviceVariant::White | DeviceVariant::Black
        ));
    }

    // Physical suite assumes exactly one device when devices are present.
    assert_eq!(
        list.len(),
        1,
        "hardware tests require exactly one device; found {}",
        list.len()
    );
}
