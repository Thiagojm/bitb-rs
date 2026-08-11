//! Public selection edge cases that do not require hardware mocks.

use bitb_rs::{BitBabbler, BitBabblerError};

#[test]
fn open_by_serial_rejects_empty() {
    let err = BitBabbler::open_by_serial("").expect_err("empty serial");
    assert_eq!(err, BitBabblerError::MissingSerial);
}
