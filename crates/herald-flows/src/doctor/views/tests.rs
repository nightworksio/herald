use super::*;

#[test]
fn a_private_route_whose_kind_is_never_sent_a_section_is_told_so() {
    assert_eq!(
        checked_note(("team", "report daily"), Checking::Unadvised, false),
        format!(
            "The route team is sent \"{WITHHELD}\" in place of the report daily's advisories: {UNADVISED}."
        )
    );
}
