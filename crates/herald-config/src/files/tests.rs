use super::*;

#[test]
fn a_path_of_names_lies_beside_the_config_and_an_absolute_one_in_the_allowed_directory_holding_it()
{
    let allowed = [
        Allowed {
            written: "/srv/herald".to_owned(),
            resolved: "/srv/herald".to_owned(),
        },
        Allowed {
            written: "/etc/herald/".to_owned(),
            resolved: "/etc/herald/".to_owned(),
        },
        Allowed {
            written: "/tmp/herald".to_owned(),
            resolved: "/private/tmp/herald".to_owned(),
        },
    ];
    for beside in ["body.md", "./body.md", "templates/body.md", "a/./b.md"] {
        assert_eq!(
            Within::of(beside, &allowed),
            Some(Within::Beside),
            "{beside}"
        );
    }
    assert_eq!(
        Within::of("/srv/herald/body.md", &allowed),
        Some(Within::Allowed("/srv/herald"))
    );
    assert_eq!(
        Within::of("/etc/herald/lists/blocks.csv", &allowed),
        Some(Within::Allowed("/etc/herald/"))
    );
    for linked in ["/tmp/herald/body.md", "/private/tmp/herald/body.md"] {
        assert_eq!(
            Within::of(linked, &allowed),
            Some(Within::Allowed("/private/tmp/herald")),
            "{linked}"
        );
    }
    for refused in [
        "",
        "../body.md",
        "templates/../../body.md",
        "/srv/herald/../secrets",
        "/srv/heraldic/body.md",
        "/proc/self/environ",
    ] {
        assert_eq!(Within::of(refused, &allowed), None, "{refused}");
    }
    assert_eq!(Within::of("/srv/herald/body.md", &[]), None);
}

#[test]
fn a_reach_holds_a_directory_inside_one_of_its_own_step_by_step() {
    let listed = Reach::Listed(vec!["/work".to_owned(), "/srv/herald/".to_owned()]);
    for held in ["/work", "/work/templates", "/srv/herald", "/srv/herald/a/b"] {
        assert!(listed.holds(held), "{held}");
    }
    for refused in [
        "/",
        "/workspace",
        "/wor",
        "/srv",
        "/srv/heraldic",
        "work",
        "",
    ] {
        assert!(!listed.holds(refused), "{refused}");
    }
    let beside = Reach::Beside("/etc/herald".to_owned());
    assert!(beside.holds("/etc/herald/templates"));
    assert!(!beside.holds("/etc"));
    assert!(!Reach::Beside(String::new()).holds("/etc"));
    assert!(!Reach::Beside("relative".to_owned()).holds("relative/dir"));
    assert!(!Reach::Named.holds("/"));
}

#[test]
fn a_reach_says_what_it_is_and_whence_and_refuses_naming_no_directory() {
    let listed = Reach::Listed(vec!["/work".to_owned(), "/srv".to_owned()]);
    assert_eq!(
        listed.said(),
        "[files] allowed may name directories inside those HERALD_FILES_ALLOWED lists on this host: /work, /srv"
    );
    assert_eq!(
        Reach::Beside("/etc/herald".to_owned()).said(),
        "[files] allowed may name directories inside the config's own, /etc/herald, alone: HERALD_FILES_ALLOWED is not set on this host"
    );
    assert_eq!(
        Reach::Named.said(),
        "[files] allowed may name no directory: this host reads files by name"
    );
    for reach in [
        listed,
        Reach::Beside("/etc/herald".to_owned()),
        Reach::Named,
    ] {
        assert!(!reach.refusal().contains('/'), "{reach:?}");
    }
}

#[test]
fn a_host_that_reads_by_name_reaches_nothing_and_resolves_nothing() {
    let read = |_: &str| Ok(String::new());
    assert_eq!(read.reach(), Reach::Named);
    assert_eq!(read.resolved("/etc"), None);
}
