# PROJECT_CONTEXT — bitb-rs

## Purpose

Provide a small synchronous Rust library to open one TNRG BitBabbler White or Black over USB and read entropy bytes, with optional explicit XOR folding.

## Users

- Application developers who need hardware entropy from a BitBabbler without running `seedd`.
- Systems that already manage WinUSB (Windows) or udev permissions (Linux).

## Main flows

1. `list_devices` / `open` / `open_by_serial`
2. `get_bits` (raw) or `get_bits_with_fold` (folds 0–4)
3. `random_u64` / `random_range` (always raw)
4. Drop handle; reopen after disconnect
5. `examples/bitb_lab.rs` for explicit manual API and throughput exercises

## Vocabulary

| Term | Meaning |
|------|---------|
| Raw | Device bytes with only FTDI status framing removed |
| Fold n | XOR-reduce `2^n` consecutive raw segments of length N |
| White / Black | Product strings `White RNG` / `Black RNG`, VID:PID `0403:7840` |

## Constraints

- Windows + Linux; `rusb` backend; MIT crate license.
- No health checks, auto-reconnect, multi-device handles, or GPL code reuse.
- `bit-babbler-0.9/` is local protocol reference only, excluded from git package.

## Validation state

- **Deterministic (Windows):** fmt, check, test, clippy `-D warnings`, doctest, MSRV 1.85.0 — passed. Hardware tests are `#[ignore]` in the default suite.
- **Physical White (Windows):** validated with
  `cargo test --test hardware -- --ignored --test-threads=1 --nocapture`
  (list/open, raw, folds 1–4, `random_u64`/`random_range`). Re-run after hardware-test reliability fixes.
- **Laboratory example:** parser/formatting are covered by `cargo test --all-targets`; physical smoke runs are manual and descriptive only, with no output gating.
- **Repository:** published publicly as `Thiagojm/bitb-rs`; crate and repository use the MIT license. The local GPL reference remains excluded.
- **Black / Linux:** not confirmed until native runs exist. Cross-compile without sysroot is not Linux validation.
