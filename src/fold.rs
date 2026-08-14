//! Fold levels and XOR helpers for explicit multi-segment folding.

use crate::BitBabblerError;

/// Explicit XOR folding depth applied only by [`crate::BitBabbler::get_bits_with_fold`].
///
/// Fold `n` reads `2^n` consecutive raw segments of the requested length and
/// combines them with successive byte-wise XOR. [`Fold::Raw`] (`0`) performs no
/// folding and is the crate default.
///
/// # Examples
///
/// ```
/// use bitb_rs::Fold;
///
/// assert_eq!(Fold::try_from(0)?, Fold::Raw);
/// assert_eq!(Fold::try_from(2)?.segment_count(), 4);
/// # Ok::<(), bitb_rs::BitBabblerError>(())
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Fold {
    /// No folding: return raw device bytes (`fold = 0`).
    #[default]
    Raw = 0,
    /// XOR two consecutive raw segments (`fold = 1`).
    One = 1,
    /// XOR four consecutive raw segments (`fold = 2`).
    Two = 2,
    /// XOR eight consecutive raw segments (`fold = 3`).
    Three = 3,
    /// XOR sixteen consecutive raw segments (`fold = 4`).
    Four = 4,
}

impl Fold {
    /// Numeric fold depth in `0..=4`.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Number of equal-sized raw segments required for this fold.
    #[must_use]
    pub const fn segment_count(self) -> usize {
        1usize << self.as_u8()
    }
}

impl TryFrom<u8> for Fold {
    type Error = BitBabblerError;

    /// Converts a numeric fold depth into a [`Fold`].
    ///
    /// # Errors
    ///
    /// Returns [`BitBabblerError::InvalidFold`] if `value` is outside `0..=4`.
    ///
    /// # Examples
    ///
    /// ```
    /// use bitb_rs::Fold;
    ///
    /// assert_eq!(Fold::try_from(1)?, Fold::One);
    /// assert!(matches!(
    ///     Fold::try_from(5),
    ///     Err(bitb_rs::BitBabblerError::InvalidFold { value: 5 })
    /// ));
    /// # Ok::<(), bitb_rs::BitBabblerError>(())
    /// ```
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            4 => Ok(Self::Four),
            other => Err(BitBabblerError::InvalidFold { value: other }),
        }
    }
}

/// XOR `src` into `dst` byte by byte. Lengths must match.
pub(crate) fn xor_into(dst: &mut [u8], src: &[u8]) {
    debug_assert_eq!(dst.len(), src.len());
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d ^= *s;
    }
}

/// Reference folding of a contiguous block with `segment_count` equal segments.
///
/// Used only in tests to prove segmented streaming XOR matches whole-block folding.
#[cfg(test)]
pub(crate) fn fold_contiguous(block: &[u8], fold: Fold) -> Vec<u8> {
    let segments = fold.segment_count();
    assert!(
        block.len() % segments == 0,
        "block length must be divisible by segment count"
    );
    let n = block.len() / segments;
    let mut out = block[..n].to_vec();
    for seg in 1..segments {
        let start = seg * n;
        xor_into(&mut out, &block[start..start + n]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Fold, fold_contiguous, xor_into};

    #[test]
    fn default_is_raw() {
        assert_eq!(Fold::default(), Fold::Raw);
        assert_eq!(Fold::Raw.as_u8(), 0);
    }

    #[test]
    fn try_from_accepts_zero_through_four() {
        for v in 0u8..=4 {
            let fold = Fold::try_from(v).expect("valid fold");
            assert_eq!(fold.as_u8(), v);
            assert_eq!(fold.segment_count(), 1usize << v);
        }
    }

    #[test]
    fn try_from_rejects_out_of_range() {
        for v in [5u8, 6, 10, 255] {
            assert!(matches!(
                Fold::try_from(v),
                Err(crate::BitBabblerError::InvalidFold { value }) if value == v
            ));
        }
    }

    #[test]
    fn segmented_xor_matches_contiguous_for_all_folds() {
        // Deterministic raw stream: 16 segments of 8 bytes for fold 4.
        let mut raw = Vec::with_capacity(16 * 8);
        for seg in 0u8..16 {
            for i in 0u8..8 {
                raw.push(seg.wrapping_mul(17).wrapping_add(i));
            }
        }

        for fold_n in 0u8..=4 {
            let fold = Fold::try_from(fold_n).unwrap();
            let segments = fold.segment_count();
            let n = 8usize;
            let block = &raw[..segments * n];

            // Contiguous whole-block fold.
            let expected = fold_contiguous(block, fold);

            // Segmented path: first segment then XOR the rest.
            let mut out = block[..n].to_vec();
            for seg in 1..segments {
                let start = seg * n;
                xor_into(&mut out, &block[start..start + n]);
            }
            assert_eq!(out, expected, "fold {fold_n}");
            assert_eq!(out.len(), n);
        }
    }
}
