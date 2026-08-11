# TODO — bitb-rs

## Feito

- Design and plan documents approved (commit `4609466`).
- Crate implementation: public API, FTDI/MPSSE, folding, mocks, README, context docs.
- Hardware tests are `#[ignore]`; only `NoDevice`/empty list skips; real USB/protocol errors fail.
- Descriptor product/serial reads preserve USB errors; final line status must be THRE|TEMT.
- Automated validation on Windows (stable + MSRV 1.85.0): fmt, check, test, clippy `-D warnings`, doc tests.
- Windows physical **White** (serial ignored suite): list/open, raw, folds 1–4, `random_*`.

## Ativo

- Physical **Black** validation when a Black unit is available.
- Linux host mock/build (and hardware if present).

## Próximos

1. Validate Black: `cargo test --test hardware -- --ignored --test-threads=1 --nocapture`.
2. Run the mock/build suite on Linux.
3. Before publish: review static libusb/LGPL distribution obligations.

## Backlog

- macOS support (explicitly out of v1).
- `rand_core` adapter (out of v1).
- Optional public tuning of bitrate/latency (rejected for v1).
