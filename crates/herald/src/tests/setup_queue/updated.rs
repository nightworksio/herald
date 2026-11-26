use super::*;

/// The note said where a signed update can vouch for no head off
/// github.com.
const UNSIGNED_HOST: &str =
    "off github.com herald knows no committer the instance signs its own commits for";

#[test]
fn an_update_never_vouched_for_asks_no_signer_or_pause_and_is_shown_before_it_is_written() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, false);
    let answers = "\nmain\n\n\n\n\n\n\n\nsquash\n\n\n\nnever\n\n\n\ny\n";
    let (done, out) = wizard(&setup, &world, &[], answers);
    assert_eq!(done, Ok(DONE), "{out}");
    let shown = out.find("wait = \"2h\"\nupdated = \"never\"\napplied_by");
    let confirmed = out.find("Write the merge queue cli-main into ");
    assert!(shown.is_some() && shown < confirmed, "{out}");
    for unasked in ["The committer ", "How long it pauses"] {
        assert!(!out.contains(unasked), "{unasked} in {out}");
    }
    let written = config_of(&setup);
    assert!(written.contains("updated = \"never\"\n"), "{written}");
    for key in ["signer", "update_wait", "role_lost"] {
        assert!(!written.contains(&format!("{key} =")), "{key} in {written}");
    }
}

#[test]
fn a_forge_s_queue_asks_what_becomes_of_a_pull_request_whose_gesture_s_maker_lost_the_role() {
    let setup = setup(CONFIG);
    let world = World::default();
    ruled(&world, true);
    let answers = "\nmain\n\n\n\n\n\n\n\nmerge\n\nstay\n\n\ny\n";
    let (done, out) = wizard(&setup, &world, &[], answers);
    assert_eq!(done, Ok(DONE), "{out}");
    for said in [
        "  take-out: herald takes it out of the forge's queue on its next run.",
        "  stay: It stays in the forge's queue.",
        "once who queued it no longer holds the least role [take-out]: ",
        "role_lost = \"stay\"\napplied_by",
    ] {
        assert!(out.contains(said), "{said} in {out}");
    }
    assert!(
        !out.contains("vouches for the head herald's own update"),
        "{out}"
    );
    assert!(config_of(&setup).contains("role_lost = \"stay\"\n"));
    let flags = [
        "--branch", "dev", "--runs", "forge", "--method", "merge", "--yes",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(config_of(&setup).ends_with(
        "branch = \"dev\"\nruns = \"forge\"\nenter = { label = \"herald:merge\" }\nleast = \"write\"\nvouches = \"gesture\"\nskew = \"5s\"\nmethod = \"merge\"\nmessage = \"forge\"\nrole_lost = \"take-out\"\napplied_by = \"command\"\nwrite = { token = { secret = \"HERALD_GITHUB_MERGE_TOKEN\" } }\n"
    ));
}

#[test]
fn an_enterprise_signer_is_asked_by_the_queue_s_own_api_and_kept_until_answered_none() {
    let config = CONFIG.replace(
        "repository = \"cli/cli\"\n",
        "repository = \"cli/cli\"\napi = \"https://ghe.example/api/v3\"\n",
    );
    let setup = setup(&config);
    let world = World::default();
    let flags = [
        "--branch",
        "main",
        "--runs",
        "herald",
        "--method",
        "squash",
        "--write-token",
        "HERALD_MERGER",
    ];
    let answers = "\n\n\n\n\n\n\n\n\n\n\nnoreply@ghe.example\n0s\n\n\ny\n";
    let (done, out) = wizard(&setup, &world, &flags, answers);
    assert_eq!(done, Ok(DONE), "{out}");
    let noted = out.find(UNSIGNED_HOST);
    let asked = out.find("vouches for the head herald's own update");
    assert!(noted.is_some() && noted < asked, "{out}");
    assert!(
        out.contains("The committer https://ghe.example/api/v3 signs the commits it makes for, which you vouch is the instance's own and no user's; none for no such committer [none]: "),
        "{out}"
    );
    assert!(!out.contains("github.com signs"), "{out}");
    assert!(config_of(&setup).contains(
        "updated = \"signed\"\nsigner = \"noreply@ghe.example\"\nupdate_wait = \"0s\"\n"
    ));
    assert!(
        !doctored(&setup, &world).contains("never lets a gesture vouch"),
        "a signer names whose signature counts"
    );
    let (kept, out) = wizard(&setup, &world, &["cli-main", "--yes"], "");
    assert_eq!(kept, Ok(DONE), "{out}");
    assert!(!out.contains(UNSIGNED_HOST), "{out}");
    let written = config_of(&setup);
    assert!(
        written.contains("signer = \"noreply@ghe.example\"\nupdate_wait = \"0s\"\n"),
        "{written}"
    );
    let (cleared, out) = wizard(
        &setup,
        &world,
        &["cli-main", "--signer", "none", "--yes"],
        "",
    );
    assert_eq!(cleared, Ok(DONE), "{out}");
    assert!(out.contains(UNSIGNED_HOST), "{out}");
    assert!(!config_of(&setup).contains("signer ="));
}

#[test]
fn where_the_forge_signs_no_update_it_is_said_before_updated_is_asked_and_no_signer_is() {
    let forgejo = CONFIG.replace(
        "kind = \"github\"\nrepository = \"cli/cli\"",
        "kind = \"forgejo\"\nrepository = \"forgejo/forgejo\"",
    );
    let setup = setup(&forgejo);
    let world = World::default();
    let flags = [
        "--branch", "main", "--runs", "herald", "--method", "merge", "--yes",
    ];
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(
        out.contains("Forgejo's API cannot tell the instance's signature from an author's"),
        "{out}"
    );
    let written = config_of(&setup);
    assert!(
        written.contains("updated = \"signed\"\nupdate_wait = \"10s\"\n"),
        "{written}"
    );
    assert!(!written.contains("signer ="), "{written}");
}
