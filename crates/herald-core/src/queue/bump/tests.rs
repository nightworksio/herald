use super::*;

fn shipped() -> Vec<BumpPattern> {
    SHIPPED_BUMPS
        .iter()
        .map(|text| BumpPattern::new(text).unwrap())
        .collect()
}

#[allow(clippy::unnecessary_wraps)]
fn bump(package: &str, to: Option<&str>) -> Option<Bump> {
    Some(Bump {
        package: package.to_owned(),
        to: to.map(str::to_owned),
    })
}

#[test]
fn the_shipped_patterns_read_dependabot_and_renovate_titles() {
    let patterns = shipped();
    let read = |title: &str| bumped(&patterns, title);
    assert_eq!(
        read("Bump serde from 1.0.1 to 1.0.2"),
        bump("serde", Some("1.0.2"))
    );
    assert_eq!(
        read("build(deps): Bump crate-ci/typos from 1.52.1 to 1.52.2 in the actions group"),
        bump("crate-ci/typos", Some("1.52.2"))
    );
    assert_eq!(
        read("chore(deps): update dependency left-pad to v2.0.1"),
        bump("left-pad", Some("2.0.1"))
    );
    assert_eq!(
        read("Update Rust crate jiff to 0.3"),
        bump("Rust crate jiff", Some("0.3"))
    );
    assert_eq!(
        read("Update actions/checkout action to v5 (major)"),
        bump("actions/checkout action", Some("5"))
    );
    assert_eq!(read("Bump the actions group with 2 updates"), None);
    assert_eq!(read("Add a queue"), None);
}

#[test]
fn a_leading_v_is_dropped_only_before_a_digit() {
    assert_eq!(version("v2.0"), "2.0");
    assert_eq!(version("V2"), "2");
    assert_eq!(version("2.0"), "2.0");
    assert_eq!(version("vnext"), "vnext");
    assert_eq!(version("v"), "v");
}

#[test]
fn a_pattern_without_to_reads_the_package_alone() {
    let pattern = BumpPattern::new("deps: refresh {package}").unwrap();
    assert_eq!(pattern.as_str(), "deps: refresh {package}");
    assert_eq!(
        pattern.read("DEPS: Refresh lockfile"),
        bump("lockfile", None)
    );
    assert_eq!(pattern.read("deps: refresh "), None);
    assert_eq!(pattern.read("deps: refres"), None);
}

#[test]
fn to_takes_the_whole_run_up_to_whitespace() {
    let pattern = BumpPattern::new("{package} -> {to}").unwrap();
    assert_eq!(pattern.read("a -> 1.2.3"), bump("a", Some("1.2.3")));
    assert_eq!(pattern.read("a -> 1.2.3 now"), None);
    assert_eq!(pattern.read("a -> "), None);
    let trailing = BumpPattern::new("{package} -> {to} *").unwrap();
    assert_eq!(trailing.read("a -> 1.2 now"), bump("a", Some("1.2")));
}

#[test]
fn the_package_is_the_shortest_run_that_lets_the_rest_match() {
    let pattern = BumpPattern::new("{package} to {to}").unwrap();
    assert_eq!(pattern.read("a to b to 3"), bump("a to b", Some("3")));
    let spaced = BumpPattern::new("{package} to {to}*").unwrap();
    assert_eq!(spaced.read("a to b to 3"), bump("a", Some("b")));
    assert_eq!(spaced.read(" to 3"), None);
}

#[test]
fn a_title_is_read_up_to_its_longest() {
    let pattern = BumpPattern::new("{package}").unwrap();
    let long = "x".repeat(LONGEST_TITLE + 10);
    assert_eq!(pattern.read(&long), bump(&"x".repeat(LONGEST_TITLE), None));
}

#[test]
fn a_pattern_names_its_package_once_and_no_other_placeholder() {
    assert_eq!(
        BumpPattern::new("Bump {to}"),
        Err(NotABumpPattern::NoPackage)
    );
    assert_eq!(
        BumpPattern::new("{package} {package}"),
        Err(NotABumpPattern::Repeated("package"))
    );
    assert_eq!(
        BumpPattern::new("{package} {to} {to}"),
        Err(NotABumpPattern::Repeated("to"))
    );
    assert_eq!(
        BumpPattern::new("{package} {from}"),
        Err(NotABumpPattern::Unknown("from".to_owned()))
    );
    assert_eq!(
        BumpPattern::new("{package} {to"),
        Err(NotABumpPattern::Unclosed),
    );
    assert_eq!(
        NotABumpPattern::NoPackage.to_string(),
        "a bump pattern names the package as {package}"
    );
    assert_eq!(
        NotABumpPattern::Repeated("to").to_string(),
        "a bump pattern names {to} once"
    );
    assert_eq!(
        NotABumpPattern::Unclosed.to_string(),
        "a bump pattern closes each {placeholder}"
    );
    assert_eq!(
        NotABumpPattern::Unknown("from".to_owned()).to_string(),
        "a bump pattern names {package} and {to}, not {from}"
    );
}

#[test]
fn many_wildcards_against_a_long_title_still_answer() {
    let pattern = BumpPattern::new("*a*a*a*a*{package}*b").unwrap();
    let title = "a".repeat(LONGEST_TITLE);
    assert_eq!(pattern.read(&title), None);
}

#[test]
fn the_first_pattern_that_reads_a_title_names_its_bump() {
    let patterns = vec![
        BumpPattern::new("Bump {package} from * to {to}").unwrap(),
        BumpPattern::new("Bump {package}*").unwrap(),
    ];
    assert_eq!(
        bumped(&patterns, "Bump a from 1 to 2"),
        bump("a", Some("2"))
    );
    assert_eq!(bumped(&patterns, "Bump b"), bump("b", None));
    assert_eq!(bumped(&[], "Bump b"), None);
}
