//! Private USB transport trait and `rusb` backend.
//!
//! The trait exists only to allow deterministic unit tests. It is not part of
//! the public API and is not a multi-backend abstraction for production.

use crate::error::BitBabblerError;
use crate::policy::{
    PRODUCT_ID, USB_ALT_SETTING, USB_CONFIGURATION, USB_INTERFACE, USB_TIMEOUT_MS, VENDOR_ID,
};

/// Bulk endpoints and max packet size validated during open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndpointConfig {
    pub ep_in: u8,
    pub ep_out: u8,
    pub max_packet: u16,
}

/// Minimum device operations required by the FTDI/MPSSE layer.
pub(crate) trait UsbHandle {
    fn set_configuration(&mut self, value: u8) -> Result<(), BitBabblerError>;
    fn claim_interface(&mut self, number: u8) -> Result<(), BitBabblerError>;
    fn release_interface(&mut self, number: u8) -> Result<(), BitBabblerError>;
    fn vendor_control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
    ) -> Result<(), BitBabblerError>;
    fn vendor_control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        len: usize,
    ) -> Result<Vec<u8>, BitBabblerError>;
    fn bulk_write(&mut self, endpoint: u8, data: &[u8]) -> Result<usize, BitBabblerError>;
    fn bulk_read(&mut self, endpoint: u8, buf: &mut [u8]) -> Result<usize, BitBabblerError>;
}

/// Identified candidate prior to full protocol initialization.
#[derive(Debug, Clone)]
pub(crate) struct EnumeratedDevice {
    pub product: String,
    pub serial: String,
    pub bus_number: u8,
    pub device_address: u8,
    pub endpoints: EndpointConfig,
    /// Opaque key used by the backend to reopen the same physical device.
    pub key: DeviceKey,
}

/// Backend-specific device identity for reopen after enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeviceKey {
    BusAddress {
        bus_number: u8,
        device_address: u8,
    },
    #[cfg(test)]
    MockId(u64),
}

/// Production handle wrapping `rusb`.
pub(crate) struct RusbHandle {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
    claimed_interface: Option<u8>,
}

impl RusbHandle {
    fn open_device(device: &rusb::Device<rusb::GlobalContext>) -> Result<Self, BitBabblerError> {
        let handle = device
            .open()
            .map_err(|e| BitBabblerError::from_rusb("open", e))?;
        Ok(Self {
            handle,
            claimed_interface: None,
        })
    }
}

impl UsbHandle for RusbHandle {
    fn set_configuration(&mut self, value: u8) -> Result<(), BitBabblerError> {
        // Detach kernel driver when present (Linux); ignore unsupported platforms.
        let _ = self.handle.set_auto_detach_kernel_driver(true);
        self.handle
            .set_active_configuration(value)
            .map_err(|e| BitBabblerError::from_rusb("set_configuration", e))
    }

    fn claim_interface(&mut self, number: u8) -> Result<(), BitBabblerError> {
        self.handle
            .claim_interface(number)
            .map_err(|e| BitBabblerError::from_rusb("claim_interface", e))?;
        self.claimed_interface = Some(number);
        Ok(())
    }

    fn release_interface(&mut self, number: u8) -> Result<(), BitBabblerError> {
        let result = self
            .handle
            .release_interface(number)
            .map_err(|e| BitBabblerError::from_rusb("release_interface", e));
        if result.is_ok() {
            self.claimed_interface = None;
        }
        result
    }

    fn vendor_control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
    ) -> Result<(), BitBabblerError> {
        // Vendor | Device | Host-to-device
        let request_type = rusb::request_type(
            rusb::Direction::Out,
            rusb::RequestType::Vendor,
            rusb::Recipient::Device,
        );
        self.handle
            .write_control(
                request_type,
                request,
                value,
                index,
                &[],
                std::time::Duration::from_millis(u64::from(USB_TIMEOUT_MS)),
            )
            .map_err(|e| BitBabblerError::from_rusb("control_out", e))?;
        Ok(())
    }

    fn vendor_control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        len: usize,
    ) -> Result<Vec<u8>, BitBabblerError> {
        let request_type = rusb::request_type(
            rusb::Direction::In,
            rusb::RequestType::Vendor,
            rusb::Recipient::Device,
        );
        let mut buf = vec![0u8; len];
        let n = self
            .handle
            .read_control(
                request_type,
                request,
                value,
                index,
                &mut buf,
                std::time::Duration::from_millis(u64::from(USB_TIMEOUT_MS)),
            )
            .map_err(|e| BitBabblerError::from_rusb("control_in", e))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn bulk_write(&mut self, endpoint: u8, data: &[u8]) -> Result<usize, BitBabblerError> {
        self.handle
            .write_bulk(
                endpoint,
                data,
                std::time::Duration::from_millis(u64::from(USB_TIMEOUT_MS)),
            )
            .map_err(|e| BitBabblerError::from_rusb("bulk_write", e))
    }

    fn bulk_read(&mut self, endpoint: u8, buf: &mut [u8]) -> Result<usize, BitBabblerError> {
        self.handle
            .read_bulk(
                endpoint,
                buf,
                std::time::Duration::from_millis(u64::from(USB_TIMEOUT_MS)),
            )
            .map_err(|e| BitBabblerError::from_rusb("bulk_read", e))
    }
}

impl Drop for RusbHandle {
    fn drop(&mut self) {
        if let Some(iface) = self.claimed_interface.take() {
            let _ = self.handle.release_interface(iface);
        }
    }
}

/// Enumerate VID:PID candidates and read product/serial strings.
pub(crate) fn enumerate_rusb() -> Result<Vec<EnumeratedDevice>, BitBabblerError> {
    let mut out = Vec::new();

    let devices = rusb::devices().map_err(|e| BitBabblerError::from_rusb("list_devices", e))?;

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(e) => return Err(BitBabblerError::from_rusb("device_descriptor", e)),
        };
        if desc.vendor_id() != VENDOR_ID || desc.product_id() != PRODUCT_ID {
            continue;
        }

        let endpoints = read_endpoint_config(&device)?;
        let handle = RusbHandle::open_device(&device)?;

        let product = read_required_usb_string(
            &handle.handle,
            desc.product_string_index(),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )?;
        let serial = read_required_usb_string(
            &handle.handle,
            desc.serial_number_string_index(),
            "missing_serial_string_index",
            "read_serial_string",
            "empty_serial_string",
        )?;

        let bus_number = device.bus_number();
        let device_address = device.address();

        // Drop the temporary open used only for string descriptors.
        drop(handle);

        out.push(EnumeratedDevice {
            product,
            serial,
            bus_number,
            device_address,
            endpoints,
            key: DeviceKey::BusAddress {
                bus_number,
                device_address,
            },
        });
    }

    Ok(out)
}

/// Open a previously enumerated device by bus/address and claim the interface.
pub(crate) fn open_rusb(
    key: &DeviceKey,
    endpoints: EndpointConfig,
) -> Result<RusbHandle, BitBabblerError> {
    let (bus_number, device_address) = match key {
        DeviceKey::BusAddress {
            bus_number,
            device_address,
        } => (*bus_number, *device_address),
        #[cfg(test)]
        DeviceKey::MockId(_) => {
            return Err(BitBabblerError::Usb {
                operation: "open",
                source: None,
            });
        }
    };

    let devices = rusb::devices().map_err(|e| BitBabblerError::from_rusb("list_devices", e))?;
    for device in devices.iter() {
        if device.bus_number() != bus_number || device.address() != device_address {
            continue;
        }
        let desc = device
            .device_descriptor()
            .map_err(|e| BitBabblerError::from_rusb("device_descriptor", e))?;
        if desc.vendor_id() != VENDOR_ID || desc.product_id() != PRODUCT_ID {
            continue;
        }

        // Re-validate endpoints against the live descriptor.
        let live = read_endpoint_config(&device)?;
        if live != endpoints {
            return Err(BitBabblerError::protocol("endpoint_config_changed"));
        }

        let mut handle = RusbHandle::open_device(&device)?;
        handle.set_configuration(USB_CONFIGURATION)?;
        handle.claim_interface(USB_INTERFACE)?;
        return Ok(handle);
    }

    Err(BitBabblerError::DeviceDisconnected)
}

/// Resolves a required USB string descriptor from index + read result.
///
/// Pure policy helper so descriptor failures are never collapsed to empty
/// strings. Used by enumeration and covered by deterministic unit tests.
pub(crate) fn resolve_required_string(
    index: Option<u8>,
    read: Result<String, rusb::Error>,
    missing_index_op: &'static str,
    read_op: &'static str,
    empty_op: &'static str,
) -> Result<String, BitBabblerError> {
    if index.is_none() {
        return Err(BitBabblerError::protocol(missing_index_op));
    }
    let raw = read.map_err(|e| BitBabblerError::from_rusb(read_op, e))?;
    let value = raw.trim().to_string();
    if value.is_empty() {
        return Err(BitBabblerError::protocol(empty_op));
    }
    Ok(value)
}

fn read_required_usb_string(
    handle: &rusb::DeviceHandle<rusb::GlobalContext>,
    index: Option<u8>,
    missing_index_op: &'static str,
    read_op: &'static str,
    empty_op: &'static str,
) -> Result<String, BitBabblerError> {
    let read = match index {
        Some(i) => handle.read_string_descriptor_ascii(i),
        None => {
            // Still go through the shared policy so missing index is consistent.
            return resolve_required_string(
                None,
                Ok(String::new()),
                missing_index_op,
                read_op,
                empty_op,
            );
        }
    };
    resolve_required_string(index, read, missing_index_op, read_op, empty_op)
}

fn read_endpoint_config(
    device: &rusb::Device<rusb::GlobalContext>,
) -> Result<EndpointConfig, BitBabblerError> {
    let config = device
        .config_descriptor(USB_CONFIGURATION.saturating_sub(1))
        .map_err(|e| BitBabblerError::from_rusb("config_descriptor", e))?;

    if config.number() != USB_CONFIGURATION {
        return Err(BitBabblerError::protocol("usb_configuration"));
    }

    let mut matched_iface = None;
    for interface in config.interfaces() {
        for alt in interface.descriptors() {
            if alt.interface_number() == USB_INTERFACE && alt.setting_number() == USB_ALT_SETTING {
                matched_iface = Some(alt);
                break;
            }
        }
        if matched_iface.is_some() {
            break;
        }
    }

    let alt = matched_iface.ok_or_else(|| BitBabblerError::protocol("usb_interface"))?;
    let endpoints: Vec<_> = alt.endpoint_descriptors().collect();
    if endpoints.len() != 2 {
        return Err(BitBabblerError::protocol("endpoint_count"));
    }

    // Official layout: endpoint[0] is IN, endpoint[1] is OUT.
    let ep0 = &endpoints[0];
    let ep1 = &endpoints[1];

    if ep0.direction() != rusb::Direction::In {
        return Err(BitBabblerError::protocol("endpoint_in_direction"));
    }
    if ep1.direction() != rusb::Direction::Out {
        return Err(BitBabblerError::protocol("endpoint_out_direction"));
    }
    if ep0.transfer_type() != rusb::TransferType::Bulk
        || ep1.transfer_type() != rusb::TransferType::Bulk
    {
        return Err(BitBabblerError::protocol("endpoint_transfer_type"));
    }

    let max_packet = ep0.max_packet_size();
    if max_packet <= 2 {
        return Err(BitBabblerError::protocol("max_packet_size"));
    }
    // Prefer the IN endpoint's max packet; OUT should match for FTDI.
    if ep1.max_packet_size() != max_packet {
        return Err(BitBabblerError::protocol("max_packet_mismatch"));
    }

    Ok(EndpointConfig {
        ep_in: ep0.address(),
        ep_out: ep1.address(),
        max_packet,
    })
}

// ---------------------------------------------------------------------------
// Deterministic mock transport (unit tests only)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod mock {
    use super::{DeviceKey, EndpointConfig, EnumeratedDevice, UsbHandle};
    use crate::error::BitBabblerError;
    use crate::policy::{PRODUCT_BLACK, PRODUCT_WHITE, USB_CONFIGURATION, USB_INTERFACE};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    #[allow(dead_code)] // fields retained for richer init/protocol assertions
    pub(crate) enum RecordedOp {
        SetConfiguration(u8),
        ClaimInterface(u8),
        ReleaseInterface(u8),
        ControlOut {
            request: u8,
            value: u16,
            index: u16,
        },
        ControlIn {
            request: u8,
            value: u16,
            index: u16,
            len: usize,
        },
        BulkWrite(Vec<u8>),
        BulkRead(usize),
    }

    #[derive(Debug, Clone)]
    pub(crate) enum MockResponse {
        Bytes(Vec<u8>),
        Err(BitBabblerError),
    }

    #[derive(Debug, Default)]
    struct MockInner {
        log: Vec<RecordedOp>,
        /// Queue of scripted responses for bulk_read (and explicit errors).
        responses: VecDeque<MockResponse>,
        /// Continuous entropy payload served by bulk_read after status framing.
        entropy: VecDeque<u8>,
        max_packet: u16,
        /// When true, each bulk_read returns FTDI status + entropy bytes.
        frame_bulk: bool,
        disconnected: bool,
        /// Optional partial bulk read sizes (popped first).
        partial_reads: VecDeque<usize>,
        /// Remaining data bytes still owed for the last MPSSE read command.
        pending_data: Option<usize>,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct MockHandle {
        inner: Arc<Mutex<MockInner>>,
    }

    impl MockHandle {
        pub(crate) fn new(max_packet: u16) -> Self {
            Self {
                inner: Arc::new(Mutex::new(MockInner {
                    max_packet,
                    frame_bulk: true,
                    ..MockInner::default()
                })),
            }
        }

        pub(crate) fn push_response(&self, response: MockResponse) {
            self.inner.lock().unwrap().responses.push_back(response);
        }

        pub(crate) fn push_entropy(&self, bytes: &[u8]) {
            self.inner
                .lock()
                .unwrap()
                .entropy
                .extend(bytes.iter().copied());
        }

        pub(crate) fn set_frame_bulk(&self, enabled: bool) {
            self.inner.lock().unwrap().frame_bulk = enabled;
        }

        pub(crate) fn set_disconnected(&self, disconnected: bool) {
            self.inner.lock().unwrap().disconnected = disconnected;
        }

        pub(crate) fn push_partial_read(&self, n: usize) {
            self.inner.lock().unwrap().partial_reads.push_back(n);
        }

        pub(crate) fn log(&self) -> Vec<RecordedOp> {
            self.inner.lock().unwrap().log.clone()
        }
    }

    impl UsbHandle for MockHandle {
        fn set_configuration(&mut self, value: u8) -> Result<(), BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::SetConfiguration(value));
            Ok(())
        }

        fn claim_interface(&mut self, number: u8) -> Result<(), BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::ClaimInterface(number));
            Ok(())
        }

        fn release_interface(&mut self, number: u8) -> Result<(), BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            g.log.push(RecordedOp::ReleaseInterface(number));
            Ok(())
        }

        fn vendor_control_out(
            &mut self,
            request: u8,
            value: u16,
            index: u16,
        ) -> Result<(), BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::ControlOut {
                request,
                value,
                index,
            });
            Ok(())
        }

        fn vendor_control_in(
            &mut self,
            request: u8,
            value: u16,
            index: u16,
            len: usize,
        ) -> Result<Vec<u8>, BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::ControlIn {
                request,
                value,
                index,
                len,
            });
            // Default modem status for GET_MODEM_STATUS (request 0x05).
            if request == 0x05 && len == 2 {
                return Ok(vec![0x00, 0x60]);
            }
            Ok(vec![0u8; len])
        }

        fn bulk_write(&mut self, _endpoint: u8, data: &[u8]) -> Result<usize, BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::BulkWrite(data.to_vec()));
            // Parse MPSSE byte-in commands so framed reads only emit the
            // exact payload length the command requested (like real hardware).
            // Command: 0x20, len_lo, len_hi, 0x87  with encoded length = len-1.
            if data.len() >= 4 {
                let mut i = 0usize;
                while i + 3 < data.len() {
                    if data[i] == 0x20 && data[i + 3] == 0x87 {
                        let encoded = u16::from(data[i + 1]) | (u16::from(data[i + 2]) << 8);
                        let nbytes = usize::from(encoded) + 1;
                        g.pending_data = Some(g.pending_data.unwrap_or(0).saturating_add(nbytes));
                        i += 4;
                        continue;
                    }
                    // Sync probe: 0xAA/0xAB + SEND_IMMEDIATE — no data payload.
                    if (data[i] == 0xAA || data[i] == 0xAB) && data[i + 1] == 0x87 {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
            }
            Ok(data.len())
        }

        fn bulk_read(&mut self, _endpoint: u8, buf: &mut [u8]) -> Result<usize, BitBabblerError> {
            let mut g = self.inner.lock().unwrap();
            if g.disconnected {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            g.log.push(RecordedOp::BulkRead(buf.len()));

            if let Some(MockResponse::Err(e)) = g.responses.front().cloned() {
                g.responses.pop_front();
                return Err(e);
            }
            if let Some(MockResponse::Bytes(b)) = g.responses.front().cloned() {
                g.responses.pop_front();
                let n = b.len().min(buf.len());
                buf[..n].copy_from_slice(&b[..n]);
                return Ok(n);
            }

            let max_packet = usize::from(g.max_packet);
            let frame = g.frame_bulk;
            let expect_modem = if max_packet == 64 {
                0x01 | 0x10 | 0x20 // MAX64 | CTS | DSR
            } else {
                0x02 | 0x10 | 0x20 // MAX512 | CTS | DSR
            };
            let line = 0x60; // THRE | TEMT

            let mut limit = buf.len();
            if let Some(n) = g.partial_reads.pop_front() {
                limit = limit.min(n);
            }

            // Cap data emission to the outstanding MPSSE command length.
            let data_budget = g.pending_data.unwrap_or(g.entropy.len());

            if !frame {
                let n = limit.min(g.entropy.len()).min(data_budget);
                for slot in buf.iter_mut().take(n) {
                    *slot = g.entropy.pop_front().unwrap_or(0);
                }
                if let Some(p) = g.pending_data.as_mut() {
                    *p = p.saturating_sub(n);
                }
                return Ok(n);
            }

            // Build FTDI framed packets into buf.
            let mut written = 0usize;
            let mut emitted_data = 0usize;
            while written + 2 <= limit && emitted_data < data_budget {
                let packet_budget = (limit - written).min(max_packet);
                if packet_budget < 2 {
                    break;
                }
                let data_room = packet_budget - 2;
                let remaining_cmd = data_budget - emitted_data;
                if g.entropy.is_empty() && written > 0 {
                    break;
                }
                if g.entropy.is_empty() || remaining_cmd == 0 {
                    // Status-only packet (empty read path).
                    buf[written] = expect_modem;
                    buf[written + 1] = line;
                    written += 2;
                    break;
                }
                let take = data_room.min(g.entropy.len()).min(remaining_cmd);
                buf[written] = expect_modem;
                buf[written + 1] = line;
                for i in 0..take {
                    buf[written + 2 + i] = g.entropy.pop_front().unwrap();
                }
                written += 2 + take;
                emitted_data += take;
                if take < data_room {
                    break;
                }
            }
            if let Some(p) = g.pending_data.as_mut() {
                *p = p.saturating_sub(emitted_data);
                if *p == 0 {
                    g.pending_data = None;
                }
            }
            Ok(written)
        }
    }

    #[derive(Debug, Clone)]
    pub(crate) struct MockCandidate {
        pub id: u64,
        pub product: String,
        pub serial: String,
        pub bus_number: u8,
        pub device_address: u8,
        pub max_packet: u16,
        pub handle: MockHandle,
    }

    impl MockCandidate {
        pub(crate) fn white(id: u64, serial: &str) -> Self {
            Self {
                id,
                product: PRODUCT_WHITE.into(),
                serial: serial.into(),
                bus_number: 1,
                device_address: id as u8,
                max_packet: 64,
                handle: MockHandle::new(64),
            }
        }

        pub(crate) fn black(id: u64, serial: &str) -> Self {
            Self {
                id,
                product: PRODUCT_BLACK.into(),
                serial: serial.into(),
                bus_number: 1,
                device_address: id as u8,
                max_packet: 512,
                handle: MockHandle::new(512),
            }
        }

        pub(crate) fn unknown(id: u64, product: &str) -> Self {
            Self {
                id,
                product: product.into(),
                serial: format!("UNK{id}"),
                bus_number: 1,
                device_address: id as u8,
                max_packet: 64,
                handle: MockHandle::new(64),
            }
        }

        pub(crate) fn to_enumerated(&self) -> EnumeratedDevice {
            EnumeratedDevice {
                product: self.product.clone(),
                serial: self.serial.clone(),
                bus_number: self.bus_number,
                device_address: self.device_address,
                endpoints: EndpointConfig {
                    ep_in: 0x81,
                    ep_out: 0x02,
                    max_packet: self.max_packet,
                },
                key: DeviceKey::MockId(self.id),
            }
        }
    }

    pub(crate) fn claim_mock(candidate: &MockCandidate) -> Result<MockHandle, BitBabblerError> {
        let mut h = candidate.handle.clone();
        h.set_configuration(USB_CONFIGURATION)?;
        h.claim_interface(USB_INTERFACE)?;
        Ok(h)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_required_string;
    use crate::error::BitBabblerError;

    #[test]
    fn product_string_rusb_error_is_preserved() {
        let err = resolve_required_string(
            Some(1),
            Err(rusb::Error::Access),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )
        .unwrap_err();
        assert_eq!(err, BitBabblerError::PermissionDenied);
    }

    #[test]
    fn serial_string_rusb_error_is_preserved() {
        let err = resolve_required_string(
            Some(2),
            Err(rusb::Error::Busy),
            "missing_serial_string_index",
            "read_serial_string",
            "empty_serial_string",
        )
        .unwrap_err();
        assert_eq!(err, BitBabblerError::DeviceBusy);
    }

    #[test]
    fn serial_timeout_maps_to_transfer_timeout() {
        let err = resolve_required_string(
            Some(2),
            Err(rusb::Error::Timeout),
            "missing_serial_string_index",
            "read_serial_string",
            "empty_serial_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::TransferTimeout {
                operation: "read_serial_string"
            }
        );
    }

    #[test]
    fn missing_product_index_is_protocol_violation() {
        let err = resolve_required_string(
            None,
            Ok("ignored".into()),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: "missing_product_string_index"
            }
        );
    }

    #[test]
    fn missing_serial_index_is_protocol_violation() {
        let err = resolve_required_string(
            None,
            Ok("ignored".into()),
            "missing_serial_string_index",
            "read_serial_string",
            "empty_serial_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: "missing_serial_string_index"
            }
        );
    }

    #[test]
    fn empty_serial_after_trim_is_protocol_violation() {
        let err = resolve_required_string(
            Some(2),
            Ok("   ".into()),
            "missing_serial_string_index",
            "read_serial_string",
            "empty_serial_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: "empty_serial_string"
            }
        );
        // MissingSerial remains reserved for open_by_serial("") argument errors.
        assert_ne!(err, BitBabblerError::MissingSerial);
    }

    #[test]
    fn empty_product_after_trim_is_protocol_violation() {
        let err = resolve_required_string(
            Some(1),
            Ok(String::new()),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: "empty_product_string"
            }
        );
    }

    #[test]
    fn valid_string_is_trimmed() {
        let value = resolve_required_string(
            Some(1),
            Ok("  White RNG  ".into()),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )
        .unwrap();
        assert_eq!(value, "White RNG");
    }

    #[test]
    fn other_usb_error_preserves_operation() {
        let err = resolve_required_string(
            Some(1),
            Err(rusb::Error::Pipe),
            "missing_product_string_index",
            "read_product_string",
            "empty_product_string",
        )
        .unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::Usb {
                operation: "read_product_string",
                source: Some(rusb::Error::Pipe),
            }
        );
    }
}
