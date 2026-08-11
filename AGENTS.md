# AGENTS.md — bitb-rs

## Reading order

1. `docs/PROJECT_CONTEXT.md` — product purpose and constraints
2. `docs/DECISIONS.md` — durable design contracts
3. `TODO.md` — current milestones and next steps
4. `docs/specs/2026-08-11-bitb-rs-design.md` — approved design
5. `docs/plans/2026-08-11-bitb-rs-plan.md` — implementation sequence

## Verified commands (run from crate root)

Deterministic suite (does not claim hardware):

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --doc
cargo +1.85.0 check --all-targets
cargo +1.85.0 test --all-targets
cargo package --allow-dirty --list
```

Physical suite (exactly one device; serial only):

```text
cargo test --test hardware -- --ignored --test-threads=1 --nocapture
```

- Default tests mark hardware as `#[ignore]`; they must not open the USB device.
- Physical runs treat only absence (`NoDevice` / empty list) as skip; permission,
  busy, protocol, USB, init, and multiple devices **fail** the test.
- Never record real device serials in docs, tests, or commits.

## Durable constraints

- Package `bitb-rs`, import `bitb_rs`; edition 2024; MSRV 1.85.0; MIT.
- Synchronous API only; no Tokio/Tauri/threads inside the crate.
- USB via `rusb` 0.9; Windows + Linux only in v1.
- One `BitBabbler` = one device; no global singleton.
- `open()` requires exactly one recognized device; otherwise `NoDevice` or `MultipleDevices`.
- Raw data is the default; folding only in `get_bits_with_fold` for folds 0–4.
- `random_u64` / `random_range` always raw; range sampling is unbiased.
- Never return partial entropy buffers.
- No health checks, stats gating, daemon, hotplug, auto-reconnect, macOS, or `rand_core`.
- Do not copy GPL text/structures from `bit-babbler-0.9/`; protocol reimplementation only.
- Do not move, edit, track, or package `bit-babbler-0.9/`.
- Do not install drivers or udev rules.
- No commit/push/deploy/driver changes without explicit user authorization.
