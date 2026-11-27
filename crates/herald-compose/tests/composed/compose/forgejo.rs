//! A Forgejo branch store opened through what the host offers: its API,
//! its repository and its token, each where given and else the run's own.

use super::*;

#[test]
fn a_forgejo_branch_store_reaches_the_api_given_the_run_s_own_or_codeberg_s() {
    let unreached = Arc::new(Hosting {
        unreached: true,
        ..github()
    });
    assert_eq!(
        kept(
            &unreached,
            "kind = \"forgejo-branch\"\nrepository = \"o/r\""
        ),
        Err("https://codeberg.org/api/v1 is out of reach".to_owned())
    );
    assert_eq!(
        kept(
            &unreached,
            "kind = \"forgejo-branch\"\nrepository = \"o/r\"\napi = \"https://forge.example/api/v1/\""
        ),
        Err("https://forge.example/api/v1 is out of reach".to_owned())
    );
    assert_eq!(
        kept(&unreached, "kind = \"forgejo-branch\""),
        Err("GITHUB_API_URL is not set; give the store's api, or run herald in the repository's Forgejo Actions".to_owned())
    );
    let mut forgejo = github();
    forgejo.unreached = true;
    forgejo.vars.insert(
        "GITHUB_API_URL".to_owned(),
        "https://forge.example/api/v1".to_owned(),
    );
    assert_eq!(
        kept(&Arc::new(forgejo), "kind = \"forgejo-branch\""),
        Err("https://forge.example/api/v1 is out of reach".to_owned())
    );
}

#[test]
fn a_forgejo_branch_store_reads_the_token_it_names_and_else_the_run_s_own() {
    let host = Arc::new(github());
    assert_eq!(
        kept(
            &host,
            "kind = \"forgejo-branch\"\nrepository = \"o/r\"\ntoken = { secret = \"VARIABLES\" }"
        ),
        Ok("kept")
    );
    let bare = Arc::new(Hosting::default());
    assert_eq!(
        kept(
            &bare,
            "kind = \"forgejo-branch\"\nrepository = \"o/r\"\ntoken = { secret = \"STATE_TOKEN\" }"
        ),
        Err(
            "STATE_TOKEN is not set; the forgejo-branch store needs a token that may push"
                .to_owned()
        )
    );
    assert_eq!(
        kept(&bare, "kind = \"forgejo-branch\"\nrepository = \"o/r\""),
        Err(
            "GITHUB_TOKEN is not set; the forgejo-branch store needs a token that may push"
                .to_owned()
        )
    );
}
