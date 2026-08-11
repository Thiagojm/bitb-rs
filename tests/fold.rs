//! Public Fold contract tests.

use bitb_rs::Fold;

#[test]
fn default_is_raw() {
    assert_eq!(Fold::default(), Fold::Raw);
}

#[test]
fn try_from_accepts_0_through_4() {
    for v in 0u8..=4 {
        let fold = Fold::try_from(v).expect("valid");
        assert_eq!(fold.as_u8(), v);
        assert_eq!(fold.segment_count(), 1usize << v);
    }
}

#[test]
fn try_from_rejects_others() {
    for v in [5u8, 7, 100, 255] {
        let err = Fold::try_from(v).unwrap_err();
        assert!(matches!(
            err,
            bitb_rs::BitBabblerError::InvalidFold { value } if value == v
        ));
    }
}
