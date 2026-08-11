# bitb-rs

Synchronous Rust access to a single [TNRG BitBabbler](https://www.bitbabbler.org/) White or Black hardware RNG over USB.

| Item | Value |
|------|--------|
| Package | `bitb-rs` |
| Import | `bitb_rs` |
| Edition | 2024 |
| MSRV | 1.85.0 |
| License | MIT |
| Platforms | Windows, Linux |
| USB backend | `rusb` 0.9 (libusb) |

## What this crate does

- Discovers devices with VID:PID `0403:7840`
- Accepts only product strings `White RNG` and `Black RNG`
- Opens **one** device per `BitBabbler` handle
- Initializes FTDI/MPSSE at the official 2.5 Mbit/s configuration
- Returns **raw** entropy by default (`get_bits`)
- Applies XOR folding only when you call `get_bits_with_fold` with folds 0–4
- Provides `random_u64` and unbiased `random_range` from **raw** bytes only

## What this crate deliberately does not do

- No health checks, ENT, FIPS, or statistical gating of output
- No daemon, sockets, or kernel entropy pool integration
- No hotplug monitor or automatic reconnection
- No multi-device handle, source mixing, or `rand_core` integration
- No public tuning of bitrate, USB latency, chunk size, or generator mask
- No driver or udev installation
- No copy of the official GPL C++ sources

## Platform setup

### Windows (WinUSB)

The device must already be associated with **WinUSB**. This crate does not install or replace drivers.

Typical flow with [Zadig](https://zadig.akeo.ie/):

1. Plug in the BitBabbler.
2. Open Zadig, enable **Options → List All Devices**.
3. Select the BitBabbler interface (`0403:7840`).
4. Bind it to **WinUSB** (not the FTDI VCP driver).
5. Confirm the app runs with permission to open that WinUSB device.

If the wrong driver owns the interface, open/list will fail with a permission or USB access error.

### Linux (udev)

The user needs permission to open USB device `0403:7840`. A restricted udev rule is the usual approach. Example (adapt group/mode to your policy):

```text
# /etc/udev/rules.d/60-bitbabbler.rules
SUBSYSTEM=="usb", ATTR{idVendor}=="0403", ATTR{idProduct}=="7840", MODE="0660", GROUP="plugdev"
```

Then:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

This crate never installs udev rules.

## Quick start

```rust
use bitb_rs::{BitBabbler, BitBabblerError, Fold};

fn main() -> Result<(), BitBabblerError> {
    // Exactly one recognized device: open it.
    // Zero → NoDevice; several → MultipleDevices (use open_by_serial).
    let mut dev = BitBabbler::open()?;

    let info = dev.device_info();
    println!("{:?} serial={}", info.variant, info.serial);

    // Raw bytes (fold 0): request bits, receive n_bits/8 bytes.
    let raw = dev.get_bits(256)?;
    assert_eq!(raw.len(), 32);

    // Explicit folding only here. Throughput scales ~1/2^fold.
    let folded = dev.get_bits_with_fold(256, Fold::One)?;
    assert_eq!(folded.len(), 32);

    let x = dev.random_u64()?;
    let y = dev.random_range(10..20)?;
    assert!((10..20).contains(&y));
    let _ = x;
    Ok(())
}
```

### Several devices

```rust
use bitb_rs::{BitBabbler, BitBabblerError};

fn main() -> Result<(), BitBabblerError> {
    for info in BitBabbler::list_devices()? {
        println!("{:?} {}", info.variant, info.serial);
    }
    let mut dev = BitBabbler::open_by_serial("YOURSERIAL")?;
    let _ = dev.get_bits(64)?;
    Ok(())
}
```

## Raw data and folding

| API | Folding |
|-----|---------|
| `get_bits(n)` | Always raw (`fold = 0`) |
| `get_bits_with_fold(n, fold)` | Fold 0–4 only; invalid values fail before I/O |
| `random_u64` / `random_range` | Always raw |

Fold `F` reads `2^F` consecutive raw segments of `N = n_bits/8` bytes and XOR-reduces them to `N` bytes. That multiplies device traffic by `2^F` and reduces throughput accordingly. Fold is **not** stored on the handle; every call is explicit.

Returned buffers never include FTDI modem/line status bytes. There is no other transformation.

## Errors and disconnection

Failures return a typed [`BitBabblerError`]. Collection methods never return partial buffers.

If the device is unplugged or a reset invalidates the handle, methods return an error such as `DeviceDisconnected`. **Discard the instance and call `open` / `open_by_serial` again.** The crate does not re-enumerate or reopen automatically.

`Drop` best-effort resets the FTDI bitmode and releases the interface; failures during drop do not panic.

## Async / Tauri / Tokio

The API is blocking and uses `&mut self` for collection. In async hosts, run calls on a blocking thread pool (`tokio::task::spawn_blocking`, Tauri `spawn_blocking`, etc.). Do not share one handle across threads without external mutual exclusion; `Sync` is not part of the public contract.

## Building and testing

### Deterministic suite (no hardware claim)

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --doc
cargo +1.85.0 check --all-targets
cargo +1.85.0 test --all-targets
```

`cargo test --all-targets` does **not** open a BitBabbler. Physical tests in
`tests/hardware.rs` are marked `#[ignore]` so the default suite stays free of
USB races and driver requirements.

### Physical suite (explicit, serial)

Requires exactly one recognized BitBabbler, with WinUSB (Windows) or USB
permission (Linux) already configured:

```text
cargo test --test hardware -- --ignored --test-threads=1 --nocapture
```

| Outcome | Meaning |
|---------|---------|
| `SKIP … no BitBabbler device present` | Only `NoDevice` / empty list — absence, not a failure |
| Test **failure** | Permission, busy, protocol, USB, init, or multiple devices |
| Test **pass** | list/open, raw, folds 1–4, and `random_*` succeeded |

Always use `--test-threads=1`. Parallel physical tests can fight over the single
interface and produce false `PermissionDenied` / busy errors.

## License

MIT. The local `bit-babbler-0.9/` tree (if present) is the official GPL package kept only as a protocol reference and is excluded from this crate package.
