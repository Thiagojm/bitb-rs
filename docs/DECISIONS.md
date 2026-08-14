# DECISIONS — bitb-rs

## 2026-08-11 — Core product contract

- **API shape:** mirror useful `intel_seed` patterns (bits in, bytes out, typed errors, `random_u64` / unbiased `random_range`) with BitBabbler-specific folding.
- **Default output:** always raw; official White fold=1 / Black fold=3 defaults are not applied.
- **Folding:** only `get_bits_with_fold`; values 0–4; per-call, not handle state.
- **Selection:** `open()` is zero→`NoDevice`, one→open, many→`MultipleDevices`; serial select is exact match.
- **Transport:** single FTDI/MPSSE path for White and Black; bitrate 2.5 Mbit/s and official pin mask stay private.
- **Backend:** `rusb` 0.9; private transport trait for mocks only (not a public multi-backend API).
- **Platforms:** Windows (WinUSB pre-bound) and Linux (user USB permission); no driver/udev install by the crate.
- **Purity:** no health checks, ENT/FIPS, daemon, hotplug, auto-reconnect, partial buffers, or GPL copy from `bit-babbler-0.9`.
- **License:** crate MIT; official tree remains GPL reference only and is package-excluded.

## 2026-08-14 — Published error contract

- **`BitBabblerError` and `DeviceInfo` are `#[non_exhaustive]`.** `Fold` and `DeviceVariant` stay exhaustive (closed sets).
- **USB/protocol discriminators are enums** (`UsbOperation`, `ProtocolOperation`), not `&'static str`.
- **`InitializationFailed` keeps the last non-fatal cause** in `source` and exposes it via `Error::source`.
- Fatal USB presence/access errors (`DeviceDisconnected`, `PermissionDenied`, `DeviceBusy`) still abort init immediately.
