use super::*;

fn tables(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_toml_file_lists_the_key_of_every_entry_of_each_table_named() {
    let text = "[[repo]]\nname = \"herald\"\n[[repo]]\nname = \"spec\"\n[[ungoverned]]\nname = \"web\"\n[[retired]]\nname = \"old\"\n";
    assert_eq!(
        names(
            "30-repos/repos.toml",
            text,
            &tables(&["repo", "ungoverned"]),
            "name"
        ),
        Ok(vec![
            "herald".to_owned(),
            "spec".to_owned(),
            "web".to_owned()
        ])
    );
    assert_eq!(
        names("REPOS.TOML", text, &tables(&["retired"]), "name"),
        Ok(vec!["old".to_owned()])
    );
}

#[test]
fn a_json_file_lists_its_own_array_or_the_arrays_named() {
    assert_eq!(
        names("repos.json", r#"[{"repo":"a"},{"repo":"b"}]"#, &[], "repo"),
        Ok(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(
        names(
            "repos.json",
            r#"{"plugins":[{"name":"p"}]}"#,
            &tables(&["plugins"]),
            "name"
        ),
        Ok(vec!["p".to_owned()])
    );
}

#[test]
fn a_member_file_that_cannot_be_read_is_named_with_what_to_do() {
    assert_eq!(
        names("repos.yaml", "", &[], "name"),
        Err(
            "\"repos.yaml\" is neither .toml nor .json; name a member file in one of them"
                .to_owned()
        )
    );
    assert_eq!(
        names("repos.toml", "[[", &tables(&["repo"]), "name"),
        Err("repos.toml cannot be read at line 1, column 3; fix it there".to_owned())
    );
    assert_eq!(
        names("repos.json", "{", &[], "name"),
        Err("repos.json cannot be read at line 1, column 1; fix it there".to_owned())
    );
    assert_eq!(
        names("repos.toml", "[[repo]]\nname = \"a\"\n", &[], "name"),
        Err("repos.toml: the file is not an array of entries; name the arrays that list the repositories with tables".to_owned())
    );
    assert_eq!(
        names("repos.toml", "[[repo]]\nname = \"a\"\n", &tables(&["missing"]), "name"),
        Err("repos.toml: missing is not an array of entries; name the arrays that list the repositories with tables".to_owned())
    );
    assert_eq!(
        names("repos.json", r#"[{"name":"a"},{"name":1}]"#, &[], "name"),
        Err("repos.json: entry 1 of the file has no name; give each entry its repository's name as name".to_owned())
    );
}
