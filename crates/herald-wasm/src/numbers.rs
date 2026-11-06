//! Numbers as they cross between Rust and JavaScript, exactly or not at all.

/// The largest whole number JavaScript counts exactly: `2^53 - 1`.
const EXACT: f64 = 9_007_199_254_740_991.0;

/// The whole number `number` is, where it is one JavaScript counts exactly.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    reason = "a whole number within ±(2^53 - 1) converts exactly"
)]
pub fn integer(number: f64) -> Option<i64> {
    (number.trunc() == number && number.abs() <= EXACT).then_some(number as i64)
}

/// `integer` as a JavaScript number, where it is one exactly.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "a whole number within ±(2^53 - 1) converts exactly"
)]
pub fn number(integer: i64) -> Option<f64> {
    (integer.unsigned_abs() <= 9_007_199_254_740_991).then_some(integer as f64)
}

/// The count `number` is, where it is a whole number no less than zero.
#[must_use]
pub fn count(number: f64) -> Option<u64> {
    integer(number).and_then(|whole| u64::try_from(whole).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_crosses_exactly_and_no_other_does() {
        assert_eq!(integer(-42.0), Some(-42));
        assert_eq!(integer(EXACT), Some(9_007_199_254_740_991));
        assert_eq!(integer(EXACT + 2.0), None);
        assert_eq!(integer(1.5), None);
        assert_eq!(integer(f64::NAN), None);
        assert_eq!(number(-7), Some(-7.0));
        assert_eq!(number(9_007_199_254_740_992), None);
        assert_eq!(count(200.0), Some(200));
        assert_eq!(count(-1.0), None);
    }
}
