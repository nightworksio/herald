use super::*;
use crate::announce::Stop;

#[test]
fn a_source_whose_members_could_not_be_read_says_why_where_it_was_told() {
    assert_eq!(
        stopped(&Stop::Store("the store is down".to_owned())),
        "its members could not be read: the store is down"
    );
    assert_eq!(
        stopped(&Stop::Source("never listed".to_owned())),
        "its members could not be read: never listed"
    );
    assert_eq!(
        stopped(&Stop::Limit("over".to_owned())),
        "its members could not be read"
    );
}
