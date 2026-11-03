//! A destination naming an option its kind lacks is refused, naming it.

use herald_config::load;

#[test]
fn a_destination_naming_an_option_its_kind_lacks_is_refused_by_name() {
    let nothing = |_: &str| Err("no file".to_owned());
    for kind in ["slack", "activitypub"] {
        let text = format!(
            "version = 1\n\n[store]\nkind = \"sqlite\"\n\n[destinations.team]\nkind = \"{kind}\"\nbogus = 1\n"
        );
        let error = load("herald.toml", &text, &nothing)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("destinations.team") && error.contains("bogus"),
            "{kind}: {error}"
        );
    }
}
