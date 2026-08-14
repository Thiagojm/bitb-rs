//! FTDI/MPSSE command sequences, status parsing, and exact raw reads.
//!
//! Behavior is reimplemented from the observed BitBabbler protocol. No GPL
//! source text or structures are copied.

use crate::error::{BitBabblerError, ProtocolOperation};
use crate::policy::{
    CLOCK_SETTLE_MS, FTDI_INIT_RETRIES, FTDI_INTERFACE_INDEX, FTDI_READ_RETRIES,
    MAX_MPSSE_READ_BYTES, MPSSE_SETTLE_MS, clock_divisor, latency_timer_ms, pin_direction_byte,
    pin_value_byte,
};
use crate::transport::{EndpointConfig, UsbHandle};

// FTDI control requests
const FTDI_SIO_RESET: u8 = 0x00;
const FTDI_SIO_SET_FLOW_CTRL: u8 = 0x02;
const FTDI_SIO_GET_MODEM_STATUS: u8 = 0x05;
const FTDI_SIO_SET_EVENT_CHAR: u8 = 0x06;
const FTDI_SIO_SET_ERROR_CHAR: u8 = 0x07;
const FTDI_SIO_SET_LATENCY_TIMER: u8 = 0x09;
const FTDI_SIO_SET_BITMODE: u8 = 0x0B;

const FTDI_SIO_RESET_SIO: u16 = 0;
const FLOW_RTS_CTS: u16 = 0x0100;
const BITMODE_RESET: u16 = 0x0000;
const BITMODE_MPSSE: u16 = 0x0200;

// Modem / line status
const FTDI_MAX64: u8 = 0x01;
const FTDI_MAX512: u8 = 0x02;
const FTDI_CTS: u8 = 0x10;
const FTDI_DSR: u8 = 0x20;
const FTDI_THRE: u8 = 0x20;
const FTDI_TEMT: u8 = 0x40;
const LINE_STATUS_OK_MASK: u8 = FTDI_THRE | FTDI_TEMT;

// MPSSE commands
const MPSSE_DATA_BYTE_IN_POS_MSB: u8 = 0x20;
const MPSSE_SET_DATABITS_LOW: u8 = 0x80;
const MPSSE_SET_DATABITS_HIGH: u8 = 0x82;
const MPSSE_NO_LOOPBACK: u8 = 0x85;
const MPSSE_SET_CLK_DIVISOR: u8 = 0x86;
const MPSSE_SEND_IMMEDIATE: u8 = 0x87;
const MPSSE_NO_CLK_DIV5: u8 = 0x8A;
const MPSSE_NO_3PHASE_CLK: u8 = 0x8D;
const MPSSE_NO_ADAPTIVE_CLK: u8 = 0x97;

/// Runtime FTDI stream state attached to an open device.
#[derive(Debug)]
pub(crate) struct FtdiSession {
    pub endpoints: EndpointConfig,
    pub expect_modem_status: u8,
    pub line_status: u8,
    /// Unconsumed bytes from the last bulk transfer for the active command.
    chunk: Vec<u8>,
    chunk_head: usize,
}

impl FtdiSession {
    pub(crate) fn new(endpoints: EndpointConfig) -> Result<Self, BitBabblerError> {
        let expect_modem_status = expected_modem_status(endpoints.max_packet)?;
        Ok(Self {
            endpoints,
            expect_modem_status,
            line_status: 0,
            chunk: Vec::new(),
            chunk_head: 0,
        })
    }

    fn chunk_remaining(&self) -> usize {
        self.chunk.len().saturating_sub(self.chunk_head)
    }

    pub(crate) fn clear_chunk(&mut self) {
        self.chunk.clear();
        self.chunk_head = 0;
    }
}

pub(crate) fn expected_modem_status(max_packet: u16) -> Result<u8, BitBabblerError> {
    let size_bit = match max_packet {
        64 => FTDI_MAX64,
        512 => FTDI_MAX512,
        _ => {
            // Accept other sizes only if they match one of the known markers
            // by using MAX64 for ≤64-ish and MAX512 otherwise is wrong; reject.
            return Err(BitBabblerError::protocol(
                ProtocolOperation::UnsupportedMaxPacket,
            ));
        }
    };
    Ok(size_bit | FTDI_CTS | FTDI_DSR)
}

/// Full device initialization: reset, MPSSE, sync, clock, pins.
pub(crate) fn initialize<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
) -> Result<(), BitBabblerError> {
    let mut last_err = None;
    for attempt in 1..=FTDI_INIT_RETRIES {
        match try_initialize_once(handle, session) {
            Ok(()) => return Ok(()),
            // Presence/access failures are not retryable init budget.
            Err(BitBabblerError::DeviceDisconnected) => {
                return Err(BitBabblerError::DeviceDisconnected);
            }
            Err(BitBabblerError::PermissionDenied) => {
                return Err(BitBabblerError::PermissionDenied);
            }
            Err(BitBabblerError::DeviceBusy) => {
                return Err(BitBabblerError::DeviceBusy);
            }
            Err(err) => {
                last_err = Some(err);
                session.clear_chunk();
                if attempt == FTDI_INIT_RETRIES {
                    break;
                }
            }
        }
    }
    Err(BitBabblerError::initialization_failed(
        FTDI_INIT_RETRIES,
        last_err.expect("init retry loop exits only after a failed attempt"),
    ))
}

fn try_initialize_once<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
) -> Result<(), BitBabblerError> {
    if !init_mpsse(handle, session)? {
        return Err(BitBabblerError::protocol(ProtocolOperation::MpsseSync));
    }

    let clk_div = clock_divisor();
    let cmd = [
        MPSSE_NO_CLK_DIV5,
        MPSSE_NO_ADAPTIVE_CLK,
        MPSSE_NO_3PHASE_CLK,
        MPSSE_SET_DATABITS_LOW,
        pin_value_byte(),
        pin_direction_byte(),
        MPSSE_SET_DATABITS_HIGH,
        0x00,
        0x00,
        MPSSE_SET_CLK_DIVISOR,
        (clk_div & 0xFF) as u8,
        (clk_div >> 8) as u8,
        MPSSE_NO_LOOPBACK,
    ];
    write_all(handle, session, &cmd)?;
    sleep_ms(CLOCK_SETTLE_MS);
    purge_read(handle, session)?;
    Ok(())
}

fn init_mpsse<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
) -> Result<bool, BitBabblerError> {
    ftdi_reset(handle)?;
    purge_read(handle, session)?;
    ftdi_set_special_chars(handle)?;
    let latency = latency_timer_ms(session.endpoints.max_packet);
    ftdi_set_latency_timer(handle, latency)?;
    ftdi_set_flow_control_rts_cts(handle)?;
    ftdi_set_bitmode(handle, BITMODE_RESET, 0)?;
    ftdi_set_bitmode(handle, BITMODE_MPSSE, 0)?;
    sleep_ms(MPSSE_SETTLE_MS);

    // Capture line status baseline from modem status control request.
    let ms = ftdi_get_modem_status(handle)?;
    session.line_status = (ms & 0xFF) as u8;

    // Sync AA and AB; allow one immediate retry of the pair before outer re-init.
    Ok(
        (check_sync(handle, session, 0xAA)? && check_sync(handle, session, 0xAB)?)
            || (check_sync(handle, session, 0xAA)? && check_sync(handle, session, 0xAB)?),
    )
}

fn check_sync<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
    cmd: u8,
) -> Result<bool, BitBabblerError> {
    let msg = [cmd, MPSSE_SEND_IMMEDIATE];
    write_all(handle, session, &msg)?;

    let mut empty_streak = 0u32;
    let max_packet = usize::from(session.endpoints.max_packet);
    let mut buf = vec![0u8; max_packet.max(512)];

    while empty_streak < FTDI_READ_RETRIES {
        let n = handle.bulk_read(session.endpoints.ep_in, &mut buf)?;
        if n == 4 && buf[2] == 0xFA && buf[3] == cmd {
            return Ok(true);
        }
        if n > 2 {
            // Unexpected data: reset streak and keep looking within budget.
            empty_streak = 0;
        } else {
            empty_streak += 1;
        }
    }
    Ok(false)
}

/// Best-effort restore of reset bitmode for Drop.
pub(crate) fn reset_bitmode_best_effort<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
) {
    let _ = purge_read(handle, session);
    let _ = ftdi_set_bitmode(handle, BITMODE_RESET, 0);
    let _ = ftdi_reset(handle);
    session.clear_chunk();
}

/// Read exactly `len` entropy bytes (status framing already stripped).
#[cfg(test)]
pub(crate) fn read_exact_raw<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
    len: usize,
) -> Result<Vec<u8>, BitBabblerError> {
    if len == 0 || len > MAX_MPSSE_READ_BYTES {
        return Err(BitBabblerError::protocol(
            ProtocolOperation::MpsseReadLength,
        ));
    }

    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| BitBabblerError::AllocationFailed {
            requested_bits: len.saturating_mul(8),
        })?;

    match read_exact_raw_into(handle, session, len, &mut out) {
        Ok(()) => {
            debug_assert_eq!(out.len(), len);
            Ok(out)
        }
        Err(e) => {
            // Never expose partial data.
            out.clear();
            session.clear_chunk();
            Err(e)
        }
    }
}

/// Read exactly `len` bytes into a caller-owned buffer that already has capacity.
pub(crate) fn read_exact_raw_into<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
    len: usize,
    out: &mut Vec<u8>,
) -> Result<(), BitBabblerError> {
    if len == 0 || len > MAX_MPSSE_READ_BYTES {
        return Err(BitBabblerError::protocol(
            ProtocolOperation::MpsseReadLength,
        ));
    }

    let start_len = out.len();
    let target = start_len + len;

    // MPSSE read command: length encoded as len-1, little-endian, then flush.
    let count = (len - 1) as u16;
    let cmd = [
        MPSSE_DATA_BYTE_IN_POS_MSB,
        (count & 0xFF) as u8,
        (count >> 8) as u8,
        MPSSE_SEND_IMMEDIATE,
    ];

    let mut reset_attempts = 0u32;
    loop {
        write_all(handle, session, &cmd)?;

        let mut empty_streak = 0u32;
        while out.len() < target {
            let need = target - out.len();
            let got = ftdi_read_payload(handle, session, need, out)?;
            if got > 0 {
                empty_streak = 0;
            } else {
                empty_streak += 1;
                if empty_streak >= FTDI_READ_RETRIES {
                    break;
                }
            }
        }

        if out.len() == target {
            // Official CHECK_EXCESS_BYTES: no leftover readahead, and final line
            // status must be exactly THRE|TEMT (not merely free of bad bits).
            if session.chunk_remaining() != 0 {
                out.truncate(start_len);
                session.clear_chunk();
                return Err(BitBabblerError::protocol(ProtocolOperation::ExcessPayload));
            }
            if session.line_status != LINE_STATUS_OK_MASK {
                out.truncate(start_len);
                session.clear_chunk();
                return Err(BitBabblerError::protocol(
                    ProtocolOperation::IncompleteLineStatus,
                ));
            }
            return Ok(());
        }

        // Incomplete: attempt limited recovery by re-init is handled by caller
        // for full device paths; here we retry command after local clear.
        out.truncate(start_len);
        session.clear_chunk();
        reset_attempts += 1;
        if reset_attempts >= FTDI_INIT_RETRIES {
            return Err(BitBabblerError::ReadRetriesExhausted {
                attempts: reset_attempts,
            });
        }
        // Re-init MPSSE for recovery.
        initialize(handle, session)?;
    }
}

/// Multi-chunk raw read for sizes that may exceed one MPSSE command.
pub(crate) fn read_raw_bytes<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
    nbytes: usize,
) -> Result<Vec<u8>, BitBabblerError> {
    let mut out = Vec::new();
    out.try_reserve_exact(nbytes)
        .map_err(|_| BitBabblerError::AllocationFailed {
            requested_bits: nbytes.saturating_mul(8),
        })?;

    let mut remaining = nbytes;
    while remaining > 0 {
        let chunk = remaining.min(MAX_MPSSE_READ_BYTES);
        let start = out.len();
        if let Err(e) = read_exact_raw_into(handle, session, chunk, &mut out) {
            out.clear();
            session.clear_chunk();
            return Err(e);
        }
        debug_assert_eq!(out.len(), start + chunk);
        remaining -= chunk;
    }
    debug_assert_eq!(out.len(), nbytes);
    Ok(out)
}

fn ftdi_read_payload<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
    need: usize,
    out: &mut Vec<u8>,
) -> Result<usize, BitBabblerError> {
    let mut copied = 0usize;

    loop {
        if session.chunk_remaining() > 0 {
            let n = drain_chunk(session, need - copied, out)?;
            copied += n;
            if copied == need || session.chunk_remaining() > 0 {
                return Ok(copied);
            }
            // Chunk exhausted; fetch more if still need data.
            if copied == need {
                return Ok(copied);
            }
        }

        // Fetch a new bulk transfer, sized to a multiple of max_packet.
        let max_packet = usize::from(session.endpoints.max_packet);
        let request = round_to_max_packet(need.saturating_sub(copied).max(1) + 2, max_packet)
            .min(MAX_MPSSE_READ_BYTES + max_packet);
        // Ensure request is at least one packet and a multiple of max_packet.
        let request = round_to_max_packet(request.max(max_packet), max_packet);

        let mut buf = vec![0u8; request];
        let xfer = handle.bulk_read(session.endpoints.ep_in, &mut buf)?;

        if xfer == 0 {
            return Ok(copied);
        }
        if xfer < 2 {
            return Ok(copied);
        }
        if xfer == 2 {
            // Status-only packet: validate and continue.
            validate_status_bytes(session, buf[0], buf[1])?;
            session.line_status = buf[1];
            return Ok(copied);
        }

        session.chunk = buf[..xfer].to_vec();
        session.chunk_head = 0;
    }
}

fn drain_chunk(
    session: &mut FtdiSession,
    need: usize,
    out: &mut Vec<u8>,
) -> Result<usize, BitBabblerError> {
    let max_packet = usize::from(session.endpoints.max_packet);
    let mut copied = 0usize;

    while session.chunk_remaining() > 0 && copied < need {
        let packet_head = session.chunk_head % max_packet;
        let mut packet_len = max_packet - packet_head;
        let mut skip = 0usize;

        match packet_head {
            0 => {
                if session.chunk_remaining() == 0 {
                    break;
                }
                let modem = session.chunk[session.chunk_head];
                if modem != session.expect_modem_status {
                    session.clear_chunk();
                    return Err(BitBabblerError::protocol(ProtocolOperation::ModemStatus));
                }
                if session.chunk_remaining() > 1 {
                    let line = session.chunk[session.chunk_head + 1];
                    if line & !LINE_STATUS_OK_MASK != 0 {
                        session.clear_chunk();
                        return Err(BitBabblerError::protocol(ProtocolOperation::LineStatus));
                    }
                    session.line_status = line;
                    skip = 2;
                } else {
                    skip = 1;
                }
            }
            1 => {
                let line = session.chunk[session.chunk_head];
                if line & !LINE_STATUS_OK_MASK != 0 {
                    session.clear_chunk();
                    return Err(BitBabblerError::protocol(ProtocolOperation::LineStatus));
                }
                session.line_status = line;
                skip = 1;
            }
            _ => {}
        }

        session.chunk_head += skip;
        packet_len = packet_len.saturating_sub(skip);

        if session.chunk_remaining() == 0 {
            break;
        }

        let n = need
            .saturating_sub(copied)
            .min(packet_len)
            .min(session.chunk_remaining());
        if n == 0 {
            // Only status was consumed in this packet slice.
            continue;
        }
        let start = session.chunk_head;
        out.extend_from_slice(&session.chunk[start..start + n]);
        session.chunk_head += n;
        copied += n;
    }

    if session.chunk_remaining() == 0 {
        session.clear_chunk();
    }
    Ok(copied)
}

fn validate_status_bytes(
    session: &FtdiSession,
    modem: u8,
    line: u8,
) -> Result<(), BitBabblerError> {
    if modem != session.expect_modem_status {
        return Err(BitBabblerError::protocol(ProtocolOperation::ModemStatus));
    }
    if line & !LINE_STATUS_OK_MASK != 0 {
        return Err(BitBabblerError::protocol(ProtocolOperation::LineStatus));
    }
    Ok(())
}

fn round_to_max_packet(n: usize, max_packet: usize) -> usize {
    if max_packet == 0 {
        return n;
    }
    n + max_packet - 1 - (n.saturating_sub(1) % max_packet)
}

fn write_all<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &FtdiSession,
    data: &[u8],
) -> Result<(), BitBabblerError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let n = handle.bulk_write(session.endpoints.ep_out, &data[offset..])?;
        if n == 0 {
            return Err(BitBabblerError::protocol(ProtocolOperation::BulkWriteZero));
        }
        offset += n;
    }
    Ok(())
}

fn purge_read<H: UsbHandle + ?Sized>(
    handle: &mut H,
    session: &mut FtdiSession,
) -> Result<usize, BitBabblerError> {
    session.clear_chunk();
    let max_packet = usize::from(session.endpoints.max_packet);
    let buf_size = round_to_max_packet(8192, max_packet);
    let mut buf = vec![0u8; buf_size];
    let mut count = 0usize;
    let mut empty_streak = 0u32;

    while empty_streak < FTDI_READ_RETRIES {
        let n = match handle.bulk_read(session.endpoints.ep_in, &mut buf) {
            Ok(n) => n,
            Err(BitBabblerError::TransferTimeout { .. }) => 0,
            Err(e) => return Err(e),
        };
        if n > 2 {
            count += n;
            empty_streak = 0;
        } else {
            empty_streak += 1;
        }
    }
    Ok(count)
}

fn ftdi_reset<H: UsbHandle + ?Sized>(handle: &mut H) -> Result<(), BitBabblerError> {
    handle.vendor_control_out(FTDI_SIO_RESET, FTDI_SIO_RESET_SIO, FTDI_INTERFACE_INDEX)
}

fn ftdi_set_bitmode<H: UsbHandle + ?Sized>(
    handle: &mut H,
    mode: u16,
    mask: u8,
) -> Result<(), BitBabblerError> {
    handle.vendor_control_out(
        FTDI_SIO_SET_BITMODE,
        mode | u16::from(mask),
        FTDI_INTERFACE_INDEX,
    )
}

fn ftdi_set_special_chars<H: UsbHandle + ?Sized>(handle: &mut H) -> Result<(), BitBabblerError> {
    // Disable event and error characters.
    handle.vendor_control_out(FTDI_SIO_SET_EVENT_CHAR, 0, FTDI_INTERFACE_INDEX)?;
    handle.vendor_control_out(FTDI_SIO_SET_ERROR_CHAR, 0, FTDI_INTERFACE_INDEX)
}

fn ftdi_set_latency_timer<H: UsbHandle + ?Sized>(
    handle: &mut H,
    ms: u8,
) -> Result<(), BitBabblerError> {
    handle.vendor_control_out(
        FTDI_SIO_SET_LATENCY_TIMER,
        u16::from(ms),
        FTDI_INTERFACE_INDEX,
    )
}

fn ftdi_set_flow_control_rts_cts<H: UsbHandle + ?Sized>(
    handle: &mut H,
) -> Result<(), BitBabblerError> {
    // Flow mode is ORed into the index field for this request.
    handle.vendor_control_out(
        FTDI_SIO_SET_FLOW_CTRL,
        0,
        FLOW_RTS_CTS | FTDI_INTERFACE_INDEX,
    )
}

fn ftdi_get_modem_status<H: UsbHandle + ?Sized>(handle: &mut H) -> Result<u16, BitBabblerError> {
    let bytes = handle.vendor_control_in(FTDI_SIO_GET_MODEM_STATUS, 0, FTDI_INTERFACE_INDEX, 2)?;
    if bytes.len() != 2 {
        return Err(BitBabblerError::protocol(
            ProtocolOperation::GetModemStatusLen,
        ));
    }
    Ok(u16::from(bytes[0]) << 8 | u16::from(bytes[1]))
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

/// Build the MPSSE read command bytes (test helper).
#[cfg(test)]
pub(crate) fn mpsse_read_command(len: usize) -> [u8; 4] {
    assert!((1..=MAX_MPSSE_READ_BYTES).contains(&len));
    let count = (len - 1) as u16;
    [
        MPSSE_DATA_BYTE_IN_POS_MSB,
        (count & 0xFF) as u8,
        (count >> 8) as u8,
        MPSSE_SEND_IMMEDIATE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::{MockHandle, MockResponse};

    fn session_64() -> FtdiSession {
        FtdiSession::new(EndpointConfig {
            ep_in: 0x81,
            ep_out: 0x02,
            max_packet: 64,
        })
        .unwrap()
    }

    fn session_512() -> FtdiSession {
        FtdiSession::new(EndpointConfig {
            ep_in: 0x81,
            ep_out: 0x02,
            max_packet: 512,
        })
        .unwrap()
    }

    #[test]
    fn mpsse_command_encodes_len_minus_one() {
        assert_eq!(mpsse_read_command(1), [0x20, 0x00, 0x00, 0x87]);
        assert_eq!(mpsse_read_command(256), [0x20, 0xFF, 0x00, 0x87]);
        assert_eq!(mpsse_read_command(65536), [0x20, 0xFF, 0xFF, 0x87]);
    }

    #[test]
    fn modem_status_for_packet_sizes() {
        assert_eq!(expected_modem_status(64).unwrap(), 0x31);
        assert_eq!(expected_modem_status(512).unwrap(), 0x32);
        assert!(expected_modem_status(128).is_err());
    }

    #[test]
    fn parse_packet_size_64_strips_status() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        // 8 data bytes → one FTDI packet of 2 status + 8 data
        let data: Vec<u8> = (0u8..8).collect();
        handle.push_entropy(&data);

        let out = read_exact_raw(&mut handle, &mut session, 8).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn parse_packet_size_512_strips_status() {
        let mut handle = MockHandle::new(512);
        let mut session = session_512();
        let data: Vec<u8> = (0u8..64).collect();
        handle.push_entropy(&data);

        let out = read_exact_raw(&mut handle, &mut session, 64).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn invalid_modem_status_errors() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        // Wrong modem status byte
        handle.push_response(MockResponse::Bytes(vec![0xFF, 0x60, 0x01, 0x02]));
        let err = read_exact_raw(&mut handle, &mut session, 2).unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::ModemStatus
            },
            "got {err:?}"
        );
    }

    #[test]
    fn invalid_line_status_errors() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        // Valid modem for 64 (0x31), bad line status with overrun bit
        handle.push_response(MockResponse::Bytes(vec![0x31, 0x02, 0xAA]));
        let err = read_exact_raw(&mut handle, &mut session, 1).unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::LineStatus
            },
            "got {err:?}"
        );
    }

    #[test]
    fn status_and_data_split_within_chunk() {
        // Within one bulk buffer, modem+line at head then data across the rest.
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        let mut packet = vec![0x31, 0x60];
        packet.extend((0u8..30).collect::<Vec<_>>());
        handle.push_response(MockResponse::Bytes(packet));
        let out = read_exact_raw(&mut handle, &mut session, 30).unwrap();
        assert_eq!(out, (0u8..30).collect::<Vec<_>>());
    }

    #[test]
    fn short_reads_assemble_exact_length() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        let data: Vec<u8> = (0u8..40).collect();
        handle.push_entropy(&data);
        // Force partial USB reads
        handle.push_partial_read(16);
        handle.push_partial_read(16);
        handle.push_partial_read(64);

        let out = read_exact_raw(&mut handle, &mut session, 40).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn timeout_maps_and_discards_partial() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        // Provide only 2 data bytes then permanent empty/timeout
        handle.push_entropy(&[0x11, 0x22]);
        for _ in 0..30 {
            handle.push_response(MockResponse::Err(BitBabblerError::TransferTimeout {
                operation: crate::UsbOperation::BulkRead,
            }));
        }
        let err = read_exact_raw(&mut handle, &mut session, 8).unwrap_err();
        assert!(matches!(
            err,
            BitBabblerError::TransferTimeout { .. }
                | BitBabblerError::ReadRetriesExhausted { .. }
                | BitBabblerError::InitializationFailed { .. }
                | BitBabblerError::ProtocolViolation { .. }
        ));
    }

    #[test]
    fn disconnect_errors() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_disconnected(true);
        let err = read_exact_raw(&mut handle, &mut session, 8).unwrap_err();
        assert_eq!(err, BitBabblerError::DeviceDisconnected);
    }

    #[test]
    fn init_sequence_issues_expected_controls() {
        let mut handle = MockHandle::new(64);

        ftdi_reset(&mut handle).unwrap();
        ftdi_set_special_chars(&mut handle).unwrap();
        ftdi_set_latency_timer(&mut handle, 2).unwrap();
        ftdi_set_flow_control_rts_cts(&mut handle).unwrap();
        ftdi_set_bitmode(&mut handle, BITMODE_RESET, 0).unwrap();
        ftdi_set_bitmode(&mut handle, BITMODE_MPSSE, 0).unwrap();

        let log = handle.log();
        let controls: Vec<_> = log
            .iter()
            .filter_map(|op| match op {
                crate::transport::mock::RecordedOp::ControlOut {
                    request,
                    value,
                    index,
                } => Some((*request, *value, *index)),
                _ => None,
            })
            .collect();

        assert!(controls.contains(&(FTDI_SIO_RESET, FTDI_SIO_RESET_SIO, FTDI_INTERFACE_INDEX)));
        assert!(controls.contains(&(FTDI_SIO_SET_EVENT_CHAR, 0, FTDI_INTERFACE_INDEX)));
        assert!(controls.contains(&(FTDI_SIO_SET_ERROR_CHAR, 0, FTDI_INTERFACE_INDEX)));
        assert!(controls.contains(&(FTDI_SIO_SET_LATENCY_TIMER, 2, FTDI_INTERFACE_INDEX)));
        assert!(controls.contains(&(
            FTDI_SIO_SET_FLOW_CTRL,
            0,
            FLOW_RTS_CTS | FTDI_INTERFACE_INDEX
        )));
        assert!(controls.contains(&(FTDI_SIO_SET_BITMODE, BITMODE_RESET, FTDI_INTERFACE_INDEX)));
        assert!(controls.contains(&(FTDI_SIO_SET_BITMODE, BITMODE_MPSSE, FTDI_INTERFACE_INDEX)));
    }

    fn significant_init_ops(
        log: &[crate::transport::mock::RecordedOp],
    ) -> Vec<crate::transport::mock::RecordedOp> {
        log.iter()
            .filter(|op| {
                matches!(
                    op,
                    crate::transport::mock::RecordedOp::ControlOut { .. }
                        | crate::transport::mock::RecordedOp::ControlIn { .. }
                        | crate::transport::mock::RecordedOp::BulkWrite(_)
                )
            })
            .cloned()
            .collect()
    }

    #[test]
    fn initialize_end_to_end_records_expected_order() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        initialize(&mut handle, &mut session).expect("initialize");
        assert_eq!(session.line_status, LINE_STATUS_OK_MASK);

        let clk_div = clock_divisor();
        let clock_cmd = vec![
            MPSSE_NO_CLK_DIV5,
            MPSSE_NO_ADAPTIVE_CLK,
            MPSSE_NO_3PHASE_CLK,
            MPSSE_SET_DATABITS_LOW,
            pin_value_byte(),
            pin_direction_byte(),
            MPSSE_SET_DATABITS_HIGH,
            0x00,
            0x00,
            MPSSE_SET_CLK_DIVISOR,
            (clk_div & 0xFF) as u8,
            (clk_div >> 8) as u8,
            MPSSE_NO_LOOPBACK,
        ];

        use crate::transport::mock::RecordedOp;
        let expected = [
            RecordedOp::ControlOut {
                request: FTDI_SIO_RESET,
                value: FTDI_SIO_RESET_SIO,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_EVENT_CHAR,
                value: 0,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_ERROR_CHAR,
                value: 0,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_LATENCY_TIMER,
                value: 2,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_FLOW_CTRL,
                value: 0,
                index: FLOW_RTS_CTS | FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_BITMODE,
                value: BITMODE_RESET,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlOut {
                request: FTDI_SIO_SET_BITMODE,
                value: BITMODE_MPSSE,
                index: FTDI_INTERFACE_INDEX,
            },
            RecordedOp::ControlIn {
                request: FTDI_SIO_GET_MODEM_STATUS,
                value: 0,
                index: FTDI_INTERFACE_INDEX,
                len: 2,
            },
            RecordedOp::BulkWrite(vec![0xAA, MPSSE_SEND_IMMEDIATE]),
            RecordedOp::BulkWrite(vec![0xAB, MPSSE_SEND_IMMEDIATE]),
            RecordedOp::BulkWrite(clock_cmd),
        ];
        assert_eq!(significant_init_ops(&handle.log()), expected);
    }

    #[test]
    fn initialize_preserves_fatal_usb_errors() {
        let cases = [
            BitBabblerError::DeviceDisconnected,
            BitBabblerError::PermissionDenied,
            BitBabblerError::DeviceBusy,
        ];
        for expected in cases {
            let mut handle = MockHandle::new(64);
            let mut session = session_64();
            match &expected {
                BitBabblerError::DeviceDisconnected => handle.set_disconnected(true),
                other => handle.set_next_control_error(other.clone()),
            }
            let err = initialize(&mut handle, &mut session).unwrap_err();
            assert_eq!(err, expected, "got {err:?}");
            assert!(
                !matches!(err, BitBabblerError::InitializationFailed { .. }),
                "fatal USB error must not be remapped to InitializationFailed"
            );
        }
    }

    #[test]
    fn check_sync_accepts_fa_echo() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        handle.push_response(MockResponse::Bytes(vec![0x31, 0x60, 0xFA, 0xAA]));
        assert!(check_sync(&mut handle, &mut session, 0xAA).unwrap());
    }

    #[test]
    fn excess_payload_detected() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        // Script a bulk response with more data bytes than the MPSSE command asked for.
        handle.set_frame_bulk(false);
        let mut packet = vec![0x31, 0x60];
        packet.extend(0u8..20);
        handle.push_response(MockResponse::Bytes(packet));
        let err = read_exact_raw(&mut handle, &mut session, 8).unwrap_err();
        assert!(matches!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::ExcessPayload
            }
        ));
    }

    #[test]
    fn incomplete_line_status_only_thre_fails_final_check() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        // Modem valid for 64, line status THRE only (0x20) — no illegal bits, but
        // not the complete THRE|TEMT (0x60) required at end of an exact read.
        let mut packet = vec![0x31, 0x20];
        packet.extend(0u8..8);
        handle.push_response(MockResponse::Bytes(packet));
        let err = read_exact_raw(&mut handle, &mut session, 8).unwrap_err();
        assert_eq!(
            err,
            BitBabblerError::ProtocolViolation {
                operation: ProtocolOperation::IncompleteLineStatus
            },
            "got {err:?}"
        );
    }

    #[test]
    fn complete_line_status_thre_temt_accepted() {
        let mut handle = MockHandle::new(64);
        let mut session = session_64();
        handle.set_frame_bulk(false);
        let mut packet = vec![0x31, 0x60];
        packet.extend(0u8..4);
        handle.push_response(MockResponse::Bytes(packet));
        let out = read_exact_raw(&mut handle, &mut session, 4).unwrap();
        assert_eq!(out, vec![0, 1, 2, 3]);
    }

    #[test]
    fn sizes_one_through_large() {
        for &n in &[1usize, 8, 9, 1024] {
            let mut handle = MockHandle::new(64);
            let mut session = session_64();
            let data: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            handle.push_entropy(&data);
            let out = read_exact_raw(&mut handle, &mut session, n).unwrap();
            assert_eq!(out.len(), n);
            assert_eq!(out, data);
        }
    }
}
