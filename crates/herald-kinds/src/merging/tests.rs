use super::*;

#[test]
fn a_maker_s_role_is_the_one_the_forge_says_and_none_below_write_or_unsaid() {
    assert_eq!(
        Holds::Role(Permission::Maintain).role(),
        Some(Permission::Maintain)
    );
    assert_eq!(Holds::Nothing.role(), None);
    assert_eq!(Holds::Unsaid.role(), None);
    let taken = TakenBack {
        number: 7,
        url: "u7".to_owned(),
        id: None,
        gestures: Vec::new(),
        cancels: Vec::new(),
        makers: BTreeMap::from([
            ("h".to_owned(), Holds::Role(Permission::Admin)),
            ("r".to_owned(), Holds::Nothing),
            ("u".to_owned(), Holds::Unsaid),
        ]),
        whole: true,
    };
    assert_eq!(taken.role_of("h"), Some(Permission::Admin));
    assert_eq!(taken.role_of("r"), None);
    assert_eq!(taken.role_of("u"), None);
    assert_eq!(taken.role_of("gone"), None);
}
