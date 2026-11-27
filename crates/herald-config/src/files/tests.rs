use super::*;

#[test]
fn a_path_of_names_lies_beside_the_config_and_an_absolute_one_in_the_allowed_directory_holding_it()
{
    let allowed = ["/srv/herald".to_owned(), "/etc/herald/".to_owned()];
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
