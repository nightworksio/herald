use super::*;

#[test]
fn a_private_route_is_told_how_herald_learns_who_may_read_its_place() {
    let note = |checking, vouched| checked_note(("team", "report daily"), checking, vouched);
    let asked = Checking::Asked {
        asks: "who may view the channel",
        requests: 3,
    };
    assert_eq!(
        note(asked, false),
        format!(
            "Before each run's first advisory send, herald asks who may view the channel, by 3 requests charged to the run's budget, once a run for its place; where anyone more may read it, or herald cannot tell, the route team is sent \"{WITHHELD}\" in place of the report daily's advisories."
        )
    );
    assert_eq!(
        note(
            Checking::Unchecked("the webhook cannot read the channel"),
            true
        ),
        "The route team is sent the report daily's advisories on the operator's word alone, as it says vouch_unchecked = true: the webhook cannot read the channel."
    );
    assert_eq!(
        note(
            Checking::Unchecked("the webhook cannot read the channel"),
            false
        ),
        format!(
            "The route team says private = true, but the webhook cannot read the channel; it is sent \"{WITHHELD}\" in place of the report daily's advisories, unless you vouch for its place with vouch_unchecked = true."
        )
    );
    assert_eq!(
        note(Checking::Unadvised, false),
        format!(
            "The route team is sent \"{WITHHELD}\" in place of the report daily's advisories: {UNADVISED}."
        )
    );
}
