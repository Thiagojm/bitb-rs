//! Private finite limits for USB I/O, initialization, and sampling.

/// USB control/bulk transfer timeout in milliseconds.
pub(crate) const USB_TIMEOUT_MS: u32 = 5_000;

/// Empty or incomplete FTDI bulk read retries before recovery/failure.
pub(crate) const FTDI_READ_RETRIES: u32 = 10;

/// Full FTDI/MPSSE initialization attempts before giving up.
pub(crate) const FTDI_INIT_RETRIES: u32 = 20;

/// Maximum rejection-sampling draws for one `random_range` call.
pub(crate) const MAX_RANGE_SAMPLES: u32 = 100;

/// Official default device bitrate in bits per second.
pub(crate) const BITRATE_BPS: u32 = 2_500_000;

/// MPSSE master clock reference used for divisor calculation (Hz).
pub(crate) const MPSSE_CLOCK_HZ: u32 = 30_000_000;

/// Maximum payload bytes per MPSSE read command.
pub(crate) const MAX_MPSSE_READ_BYTES: usize = 65_536;

/// Settle delay after switching into MPSSE mode (milliseconds).
pub(crate) const MPSSE_SETTLE_MS: u64 = 50;

/// Settle delay after clock/pin configuration (milliseconds).
pub(crate) const CLOCK_SETTLE_MS: u64 = 30;

/// Official generator enable mask: all four generators enabled (White);
/// Black ignores unused pins under the same configuration.
pub(crate) const GENERATOR_ENABLE_MASK: u8 = 0x0f;

/// Disable-polarity nibble applied to generator pins (official default: 0).
pub(crate) const GENERATOR_DISABLE_POLARITY: u8 = 0x00;

/// USB configuration value required by the device.
pub(crate) const USB_CONFIGURATION: u8 = 1;

/// Interface number claimed for FTDI channel A.
pub(crate) const USB_INTERFACE: u8 = 0;

/// Alternate setting for the claimed interface.
pub(crate) const USB_ALT_SETTING: u8 = 0;

/// FTDI interface index for channel A control requests.
pub(crate) const FTDI_INTERFACE_INDEX: u16 = 1;

/// BitBabbler USB vendor ID.
pub(crate) const VENDOR_ID: u16 = 0x0403;

/// BitBabbler USB product ID (White and Black).
pub(crate) const PRODUCT_ID: u16 = 0x7840;

/// Product string for the White variant.
pub(crate) const PRODUCT_WHITE: &str = "White RNG";

/// Product string for the Black variant.
pub(crate) const PRODUCT_BLACK: &str = "Black RNG";

/// Compute the FTDI latency timer from max packet size and bitrate.
pub(crate) fn latency_timer_ms(max_packet: u16) -> u8 {
    let raw = u32::from(max_packet)
        .saturating_mul(8_000)
        .checked_div(BITRATE_BPS)
        .unwrap_or(0)
        .saturating_add(2);
    raw.clamp(1, 255) as u8
}

/// MPSSE clock divisor for the official bitrate.
pub(crate) fn clock_divisor() -> u16 {
    let div = MPSSE_CLOCK_HZ / BITRATE_BPS;
    (div - 1) as u16
}

/// Pin direction byte: CLK/DO/CS outputs plus disabled generators as outputs.
pub(crate) fn pin_direction_byte() -> u8 {
    let enable_mask_nibble = (!GENERATOR_ENABLE_MASK).wrapping_shl(4) & 0xf0;
    0x0B | enable_mask_nibble
}

/// Pin value byte: CLK/DO/CS low with configured generator polarity.
pub(crate) fn pin_value_byte() -> u8 {
    GENERATOR_DISABLE_POLARITY.wrapping_shl(4) & 0xf0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_divisor_is_eleven_for_2_5_mbit() {
        // 30_000_000 / 2_500_000 - 1 = 11
        assert_eq!(clock_divisor(), 11);
    }

    #[test]
    fn latency_for_common_packet_sizes() {
        assert_eq!(latency_timer_ms(64), 2);
        assert_eq!(latency_timer_ms(512), 3);
    }

    #[test]
    fn default_pin_config_matches_official_defaults() {
        // enable_mask 0x0f → inverted high nibble 0; direction 0x0B; value 0x00
        assert_eq!(pin_direction_byte(), 0x0B);
        assert_eq!(pin_value_byte(), 0x00);
    }

    #[test]
    fn constants_are_private_policy_not_public_api() {
        // Sanity: finite positive budgets stay non-zero and match protocol limits.
        const {
            assert!(USB_TIMEOUT_MS > 0);
            assert!(FTDI_READ_RETRIES > 0);
            assert!(FTDI_INIT_RETRIES > 0);
            assert!(MAX_RANGE_SAMPLES > 0);
            assert!(MAX_MPSSE_READ_BYTES == 65_536);
        }
    }
}
