//! A route's `withdraw`, keeping a deleted release's posts unless it says
//! to delete them (ADR-0048).

use herald_config::load;
use herald_core::Withdraw;

fn loaded(withdraw: &str) -> Result<Withdraw, String> {
    let text = format!(
        "version = 1\n[store]\nkind = \"sqlite\"\n[destinations.discord]\nkind = \"discord\"\ntoken = {{ secret = \"T\" }}\n[sources.gate]\nkind = \"github\"\nrepository = \"o/r\"\n[[routes]]\nid = \"releases\"\nsources = [\"gate\"]\ndestination = \"discord\"\nchannel = \"1566200000000000100\"\ntemplate = \"t\"\n{withdraw}\n[templates.t]\ntitle = \"t\"\nbody = \"b\"\n"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::common::kinds(),
    )
    .map(|config| config.routes[0].route.delivery.withdraw)
    .map_err(|error| error.to_string())
}

#[test]
fn a_route_keeps_a_deleted_release_s_posts_unless_it_says_to_delete_them() {
    assert_eq!(loaded(""), Ok(Withdraw::Keep));
    assert_eq!(loaded("withdraw = \"keep\""), Ok(Withdraw::Keep));
    assert_eq!(loaded("withdraw = \"delete\""), Ok(Withdraw::Delete));
    let refused = loaded("withdraw = \"erase\"").unwrap_err();
    assert!(refused.contains("withdraw"), "{refused}");
}
