//! Public device handle, discovery, and random data API.

use std::ops::Range;

use crate::error::BitBabblerError;
use crate::fold::{Fold, xor_into};
use crate::policy::{MAX_MPSSE_READ_BYTES, MAX_RANGE_SAMPLES, PRODUCT_BLACK, PRODUCT_WHITE};
use crate::protocol::{self, FtdiSession};
use crate::transport::{self, EnumeratedDevice, UsbHandle};

/// BitBabbler hardware variant identified by the USB product string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceVariant {
    /// TNRG BitBabbler White (`White RNG`).
    White,
    /// TNRG BitBabbler Black (`Black RNG`).
    Black,
}

/// Snapshot of a recognized BitBabbler device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Hardware variant.
    pub variant: DeviceVariant,
    /// USB serial number string.
    pub serial: String,
    /// USB product string.
    pub product: String,
    /// USB bus number at enumeration time.
    pub bus_number: u8,
    /// USB device address at enumeration time.
    pub device_address: u8,
}

/// Synchronous handle to a single open BitBabbler White or Black device.
///
/// Operations that consume entropy take `&mut self`. The type is not `Sync`.
/// On disconnect or a reset that invalidates the USB handle, methods return a
/// typed error and the consumer must open a new instance.
pub struct BitBabbler {
    handle: Box<dyn UsbHandle + Send>,
    session: FtdiSession,
    info: DeviceInfo,
}

impl std::fmt::Debug for BitBabbler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitBabbler")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl BitBabbler {
    /// Lists currently attached BitBabbler White and Black devices.
    ///
    /// Devices with VID:PID `0403:7840` and an unrecognized product string
    /// produce [`BitBabblerError::UnsupportedProduct`] instead of being omitted.
    pub fn list_devices() -> Result<Vec<DeviceInfo>, BitBabblerError> {
        let enumerated = transport::enumerate_rusb()?;
        let mut out = Vec::with_capacity(enumerated.len());
        for dev in enumerated {
            out.push(device_info_from_enumerated(&dev)?);
        }
        Ok(out)
    }

    /// Opens the only attached recognized BitBabbler.
    ///
    /// - no device → [`BitBabblerError::NoDevice`]
    /// - one device → opens it
    /// - multiple → [`BitBabblerError::MultipleDevices`]
    pub fn open() -> Result<Self, BitBabblerError> {
        let candidates = recognized_candidates(transport::enumerate_rusb()?)?;
        match candidates.len() {
            0 => Err(BitBabblerError::NoDevice),
            1 => open_enumerated(&candidates[0]),
            n => Err(BitBabblerError::MultipleDevices { count: n }),
        }
    }

    /// Opens the recognized BitBabbler with an exact serial match.
    pub fn open_by_serial(serial: &str) -> Result<Self, BitBabblerError> {
        if serial.is_empty() {
            return Err(BitBabblerError::MissingSerial);
        }
        let enumerated = transport::enumerate_rusb()?;
        // Surface unsupported products even when searching by serial.
        let mut found_unsupported = None;
        let mut match_dev = None;
        for dev in enumerated {
            match classify_product(&dev.product) {
                ProductClass::Unsupported => {
                    if dev.serial == serial {
                        found_unsupported = Some(dev.product.clone());
                    }
                }
                ProductClass::White | ProductClass::Black => {
                    if dev.serial == serial {
                        match_dev = Some(dev);
                        break;
                    }
                }
            }
        }
        if let Some(product) = found_unsupported {
            return Err(BitBabblerError::UnsupportedProduct { product });
        }
        let dev = match_dev.ok_or_else(|| BitBabblerError::DeviceNotFound {
            serial: serial.to_string(),
        })?;
        open_enumerated(&dev)
    }

    /// Returns metadata captured when the device was opened.
    #[must_use]
    pub fn device_info(&self) -> &DeviceInfo {
        &self.info
    }

    /// Reads `n_bits` of raw entropy (`fold = 0`).
    ///
    /// Equivalent to [`Self::get_bits_with_fold`] with [`Fold::Raw`].
    pub fn get_bits(&mut self, n_bits: usize) -> Result<Vec<u8>, BitBabblerError> {
        self.get_bits_with_fold(n_bits, Fold::Raw)
    }

    /// Reads entropy and applies the requested fold depth.
    ///
    /// - `n_bits` must be positive and divisible by 8
    /// - returns exactly `n_bits / 8` bytes
    /// - fold is per-call only and is never stored on the handle
    /// - on failure, no partial buffer is returned
    pub fn get_bits_with_fold(
        &mut self,
        n_bits: usize,
        fold: Fold,
    ) -> Result<Vec<u8>, BitBabblerError> {
        if n_bits == 0 {
            return Err(BitBabblerError::ZeroBitLength);
        }
        if n_bits % 8 != 0 {
            return Err(BitBabblerError::BitLengthNotByteAligned {
                requested_bits: n_bits,
            });
        }

        let n_bytes = n_bits / 8;
        let segments = fold.segment_count();

        // Overflow guard: total raw bytes = n_bytes * segments
        let total_raw = n_bytes
            .checked_mul(segments)
            .ok_or(BitBabblerError::AllocationFailed {
                requested_bits: n_bits,
            })?;
        let _ = total_raw;

        let mut out = Vec::new();
        out.try_reserve_exact(n_bytes)
            .map_err(|_| BitBabblerError::AllocationFailed {
                requested_bits: n_bits,
            })?;
        out.resize(n_bytes, 0);

        match self.read_folded_segments(&mut out, segments) {
            Ok(()) => Ok(out),
            Err(e) => {
                // Ensure partial data is not observable (already owned locally).
                out.clear();
                self.session.clear_chunk();
                Err(e)
            }
        }
    }

    /// Returns 8 raw bytes interpreted as a little-endian `u64`.
    pub fn random_u64(&mut self) -> Result<u64, BitBabblerError> {
        let bytes = self.get_bits(64)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&bytes);
        Ok(u64::from_le_bytes(arr))
    }

    /// Uniform value in the semi-open range `[start, end)` using raw bytes only.
    ///
    /// Uses rejection sampling to avoid modulo bias. Empty or inverted ranges
    /// fail before any device I/O.
    pub fn random_range(&mut self, range: Range<u64>) -> Result<u64, BitBabblerError> {
        let start = range.start;
        let end = range.end;
        if start >= end {
            return Err(BitBabblerError::InvalidRange { start, end });
        }

        let span = end - start;
        let threshold = span.wrapping_neg() % span;

        for _ in 1..=MAX_RANGE_SAMPLES {
            let sample = self.random_u64()?;
            if sample >= threshold {
                return Ok(start + (sample % span));
            }
        }

        Err(BitBabblerError::RangeSamplingExhausted {
            attempts: MAX_RANGE_SAMPLES,
        })
    }

    fn read_folded_segments(
        &mut self,
        out: &mut [u8],
        segments: usize,
    ) -> Result<(), BitBabblerError> {
        let n = out.len();
        // First segment.
        let first = self.read_raw_bytes(n)?;
        out.copy_from_slice(&first);

        if segments == 1 {
            return Ok(());
        }

        let mut tmp = Vec::new();
        let tmp_cap = n.min(MAX_MPSSE_READ_BYTES);
        tmp.try_reserve_exact(tmp_cap)
            .map_err(|_| BitBabblerError::AllocationFailed {
                requested_bits: n.saturating_mul(8),
            })?;

        for _ in 1..segments {
            let mut remaining = n;
            let mut offset = 0usize;
            while remaining > 0 {
                let piece = remaining.min(MAX_MPSSE_READ_BYTES);
                tmp.clear();
                self.read_raw_bytes_into(piece, &mut tmp)?;
                xor_into(&mut out[offset..offset + piece], &tmp);
                offset += piece;
                remaining -= piece;
            }
        }
        Ok(())
    }

    fn read_raw_bytes(&mut self, nbytes: usize) -> Result<Vec<u8>, BitBabblerError> {
        protocol::read_raw_bytes(&mut *self.handle, &mut self.session, nbytes)
    }

    fn read_raw_bytes_into(
        &mut self,
        nbytes: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), BitBabblerError> {
        let mut remaining = nbytes;
        while remaining > 0 {
            let chunk = remaining.min(MAX_MPSSE_READ_BYTES);
            protocol::read_exact_raw_into(&mut *self.handle, &mut self.session, chunk, out)?;
            remaining -= chunk;
        }
        Ok(())
    }
}

impl Drop for BitBabbler {
    fn drop(&mut self) {
        protocol::reset_bitmode_best_effort(&mut *self.handle, &mut self.session);
        let _ = self.handle.release_interface(crate::policy::USB_INTERFACE);
    }
}

enum ProductClass {
    White,
    Black,
    Unsupported,
}

fn classify_product(product: &str) -> ProductClass {
    if product == PRODUCT_WHITE {
        ProductClass::White
    } else if product == PRODUCT_BLACK {
        ProductClass::Black
    } else {
        ProductClass::Unsupported
    }
}

fn device_info_from_enumerated(dev: &EnumeratedDevice) -> Result<DeviceInfo, BitBabblerError> {
    let variant = match classify_product(&dev.product) {
        ProductClass::White => DeviceVariant::White,
        ProductClass::Black => DeviceVariant::Black,
        ProductClass::Unsupported => {
            return Err(BitBabblerError::UnsupportedProduct {
                product: dev.product.clone(),
            });
        }
    };
    Ok(DeviceInfo {
        variant,
        serial: dev.serial.clone(),
        product: dev.product.clone(),
        bus_number: dev.bus_number,
        device_address: dev.device_address,
    })
}

/// Filters enumeration to recognized devices; fails on unsupported VID:PID peers.
fn recognized_candidates(
    enumerated: Vec<EnumeratedDevice>,
) -> Result<Vec<EnumeratedDevice>, BitBabblerError> {
    let mut out = Vec::new();
    for dev in enumerated {
        match classify_product(&dev.product) {
            ProductClass::White | ProductClass::Black => out.push(dev),
            ProductClass::Unsupported => {
                return Err(BitBabblerError::UnsupportedProduct {
                    product: dev.product,
                });
            }
        }
    }
    Ok(out)
}

fn open_enumerated(dev: &EnumeratedDevice) -> Result<BitBabbler, BitBabblerError> {
    let info = device_info_from_enumerated(dev)?;
    let handle = transport::open_rusb(&dev.key, dev.endpoints)?;
    let mut session = FtdiSession::new(dev.endpoints)?;
    let mut handle = handle;
    protocol::initialize(&mut handle, &mut session)?;
    Ok(BitBabbler {
        handle: Box::new(handle),
        session,
        info,
    })
}

// ---------------------------------------------------------------------------
// Test helpers and selection / API unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::transport::mock::{self, MockCandidate};

    pub(crate) fn open_mock_initialized(
        candidate: &MockCandidate,
    ) -> Result<BitBabbler, BitBabblerError> {
        let info = DeviceInfo {
            variant: match classify_product(&candidate.product) {
                ProductClass::White => DeviceVariant::White,
                ProductClass::Black => DeviceVariant::Black,
                ProductClass::Unsupported => {
                    return Err(BitBabblerError::UnsupportedProduct {
                        product: candidate.product.clone(),
                    });
                }
            },
            serial: candidate.serial.clone(),
            product: candidate.product.clone(),
            bus_number: candidate.bus_number,
            device_address: candidate.device_address,
        };

        let handle = mock::claim_mock(candidate)?;
        let endpoints = crate::transport::EndpointConfig {
            ep_in: 0x81,
            ep_out: 0x02,
            max_packet: candidate.max_packet,
        };
        let session = FtdiSession::new(endpoints)?;
        Ok(BitBabbler {
            handle: Box::new(handle),
            session,
            info,
        })
    }

    pub(crate) fn select_from_candidates(
        candidates: &[MockCandidate],
        serial: Option<&str>,
    ) -> Result<DeviceInfo, BitBabblerError> {
        if let Some(serial) = serial {
            if serial.is_empty() {
                return Err(BitBabblerError::MissingSerial);
            }
            let enumerated: Vec<_> = candidates.iter().map(|c| c.to_enumerated()).collect();
            for dev in &enumerated {
                match classify_product(&dev.product) {
                    ProductClass::Unsupported if dev.serial == serial => {
                        return Err(BitBabblerError::UnsupportedProduct {
                            product: dev.product.clone(),
                        });
                    }
                    ProductClass::White | ProductClass::Black if dev.serial == serial => {
                        return device_info_from_enumerated(dev);
                    }
                    _ => {}
                }
            }
            return Err(BitBabblerError::DeviceNotFound {
                serial: serial.to_string(),
            });
        }

        let enumerated: Vec<_> = candidates.iter().map(|c| c.to_enumerated()).collect();
        let recognized = recognized_candidates(enumerated)?;
        match recognized.len() {
            0 => Err(BitBabblerError::NoDevice),
            1 => device_info_from_enumerated(&recognized[0]),
            n => Err(BitBabblerError::MultipleDevices { count: n }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockCandidate;

    #[test]
    fn selection_none() {
        let err = test_support::select_from_candidates(&[], None).unwrap_err();
        assert_eq!(err, BitBabblerError::NoDevice);
    }

    #[test]
    fn selection_one_white() {
        let c = [MockCandidate::white(1, "W1")];
        let info = test_support::select_from_candidates(&c, None).unwrap();
        assert_eq!(info.variant, DeviceVariant::White);
        assert_eq!(info.serial, "W1");
    }

    #[test]
    fn selection_multiple() {
        let c = [MockCandidate::white(1, "W1"), MockCandidate::black(2, "B1")];
        let err = test_support::select_from_candidates(&c, None).unwrap_err();
        assert_eq!(err, BitBabblerError::MultipleDevices { count: 2 });
    }

    #[test]
    fn selection_by_serial() {
        let c = [MockCandidate::white(1, "W1"), MockCandidate::black(2, "B1")];
        let info = test_support::select_from_candidates(&c, Some("B1")).unwrap();
        assert_eq!(info.variant, DeviceVariant::Black);
    }

    #[test]
    fn selection_serial_missing() {
        let c = [MockCandidate::white(1, "W1")];
        let err = test_support::select_from_candidates(&c, Some("NOPE")).unwrap_err();
        assert!(matches!(err, BitBabblerError::DeviceNotFound { .. }));
    }

    #[test]
    fn selection_serial_empty() {
        let c = [MockCandidate::white(1, "W1")];
        let err = test_support::select_from_candidates(&c, Some("")).unwrap_err();
        assert_eq!(err, BitBabblerError::MissingSerial);
    }

    #[test]
    fn unsupported_product_rejected() {
        let c = [MockCandidate::unknown(1, "Purple RNG")];
        let err = test_support::select_from_candidates(&c, None).unwrap_err();
        assert!(matches!(
            err,
            BitBabblerError::UnsupportedProduct { product } if product == "Purple RNG"
        ));
    }

    #[test]
    fn get_bits_validates_before_io() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        assert_eq!(dev.get_bits(0).unwrap_err(), BitBabblerError::ZeroBitLength);
        assert_eq!(
            dev.get_bits(7).unwrap_err(),
            BitBabblerError::BitLengthNotByteAligned { requested_bits: 7 }
        );
        // Invalid fold never reaches transport (Fold::try_from).
        assert!(matches!(
            Fold::try_from(5).unwrap_err(),
            BitBabblerError::InvalidFold { value: 5 }
        ));
    }

    #[test]
    fn get_bits_raw_exact_sizes() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        for &bits in &[8usize, 64, 72, 8192] {
            let n = bits / 8;
            let data: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            // Access mock handle entropy via candidate
            c.handle.push_entropy(&data);
            let out = dev.get_bits(bits).unwrap();
            assert_eq!(out.len(), n);
            assert_eq!(out, data);
        }
    }

    #[test]
    fn get_bits_with_fold_all_levels() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        let n = 8usize;
        for fold_n in 0u8..=4 {
            let fold = Fold::try_from(fold_n).unwrap();
            let segments = fold.segment_count();
            let mut raw = Vec::with_capacity(segments * n);
            for seg in 0..segments {
                for i in 0..n {
                    raw.push((seg * 17 + i) as u8);
                }
            }
            c.handle.push_entropy(&raw);
            let out = dev.get_bits_with_fold(n * 8, fold).unwrap();
            assert_eq!(out.len(), n);
            let expected = crate::fold::fold_contiguous(&raw, fold);
            assert_eq!(out, expected, "fold {fold_n}");
        }
    }

    #[test]
    fn failure_discards_partial() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        c.handle.push_entropy(&[1, 2, 3, 4]);
        c.handle.set_disconnected(true);
        // After a few successful bytes mid-read, disconnect — get_bits should err
        // without returning data. Force disconnect on next op after partial entropy
        // is insufficient for full request.
        c.handle.set_disconnected(false);
        c.handle.push_entropy(&[1, 2]);
        // Subsequent reads: disconnect
        use crate::transport::mock::MockResponse;
        c.handle
            .push_response(MockResponse::Err(BitBabblerError::DeviceDisconnected));
        let err = dev.get_bits(64).unwrap_err();
        assert!(matches!(
            err,
            BitBabblerError::DeviceDisconnected
                | BitBabblerError::ReadRetriesExhausted { .. }
                | BitBabblerError::InitializationFailed { .. }
                | BitBabblerError::ProtocolViolation { .. }
                | BitBabblerError::TransferTimeout { .. }
        ));
    }

    #[test]
    fn random_u64_is_raw_le() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        c.handle.push_entropy(&bytes);
        let v = dev.random_u64().unwrap();
        assert_eq!(v, u64::from_le_bytes(bytes));
    }

    #[test]
    fn random_range_width_one() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        c.handle.push_entropy(&[0; 8]);
        assert_eq!(dev.random_range(42..43).unwrap(), 42);
    }

    #[test]
    fn random_range_power_of_two() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        // sample 15 → 15 % 8 = 7, start 100 → 107
        let mut bytes = 15u64.to_le_bytes().to_vec();
        // provide plenty
        for _ in 0..5 {
            bytes.extend_from_slice(&15u64.to_le_bytes());
        }
        c.handle.push_entropy(&bytes);
        let v = dev.random_range(100..108).unwrap();
        assert!((100..108).contains(&v));
    }

    #[test]
    fn random_range_invalid() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        assert_eq!(
            dev.random_range(5..5).unwrap_err(),
            BitBabblerError::InvalidRange { start: 5, end: 5 }
        );
        let inverted = std::ops::Range { start: 9, end: 3 };
        assert_eq!(
            dev.random_range(inverted).unwrap_err(),
            BitBabblerError::InvalidRange { start: 9, end: 3 }
        );
    }

    #[test]
    fn random_range_near_max() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        // Provide samples that always accept
        for _ in 0..8 {
            c.handle.push_entropy(&u64::MAX.to_le_bytes());
        }
        let v = dev.random_range((u64::MAX - 4)..u64::MAX).unwrap();
        assert!(((u64::MAX - 4)..u64::MAX).contains(&v));
    }

    #[test]
    fn random_range_exhaustion() {
        let c = MockCandidate::white(1, "W1");
        let mut dev = test_support::open_mock_initialized(&c).unwrap();
        // span=3, threshold = (-3)%3 = 1 (wrapping). Samples < 1 are rejected.
        // Use sample 0 repeatedly.
        let mut bytes = Vec::new();
        for _ in 0..(MAX_RANGE_SAMPLES as usize + 5) {
            bytes.extend_from_slice(&0u64.to_le_bytes());
        }
        c.handle.push_entropy(&bytes);
        let err = dev.random_range(0..3).unwrap_err();
        // May exhaust sampling or succeed if 0 >= threshold.
        // threshold for span 3: wrapping_neg(3)%3 = (-3 as u64)%3.
        // 3u64.wrapping_neg() = 2^64-3; (2^64-3)%3 = 0 because 2^64%3=1?
        // 2^64 ≡ 1 (mod 3) since 2≡-1, 2^64≡1. So 2^64-3 ≡ 1-0 ≡ 1 (mod 3)?
        // 3≡0 so 2^64-3 ≡ 1 (mod 3). threshold=1.
        // sample 0 < 1 → reject. Good.
        assert_eq!(
            err,
            BitBabblerError::RangeSamplingExhausted {
                attempts: MAX_RANGE_SAMPLES
            }
        );
    }

    #[test]
    fn drop_after_disconnect_no_panic() {
        let c = MockCandidate::white(1, "W1");
        let dev = test_support::open_mock_initialized(&c).unwrap();
        c.handle.set_disconnected(true);
        drop(dev);
    }

    #[test]
    fn open_by_serial_empty_public() {
        let err = BitBabbler::open_by_serial("").unwrap_err();
        assert_eq!(err, BitBabblerError::MissingSerial);
    }
}
