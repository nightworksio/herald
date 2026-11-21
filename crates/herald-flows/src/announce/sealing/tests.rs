use super::*;

#[test]
fn an_outcome_already_sealed_is_told_the_same_again() {
    for stopped in [true, false] {
        assert_eq!(
            sealed(Outcome::Sealed { stopped }),
            Outcome::Sealed { stopped }
        );
    }
}
