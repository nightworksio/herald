use super::*;

/// GitHub's answers on `main` of cli/cli: a merge queue merging by
/// squash, and the repository allowing a squash, and a rebase where
/// `rebases`.
fn squashing(world: &World, rebases: bool) {
    let answers = [
        (
            "/repos/cli/cli",
            json!({"private": false, "owner": {"type": "Organization"},
                "allow_merge_commit": false, "allow_squash_merge": true, "allow_rebase_merge": rebases}),
        ),
        (
            "/repos/cli/cli/branches/main",
            json!({"commit": {"sha": "b1"}, "protected": true}),
        ),
        (
            "/repos/cli/cli/rules/branches/main?per_page=100&page=1",
            json!([{"type": "merge_queue", "parameters": {"merge_method": "SQUASH"}}]),
        ),
    ];
    let forge = &mut world.state.lock().unwrap().forge;
    for (path, body) in answers {
        forge.insert(path.to_owned(), (200, body.to_string()));
    }
}

#[test]
fn the_forge_s_queue_method_and_the_repository_s_methods_are_said_and_narrow_the_offer() {
    let setup = setup(CONFIG);
    let world = World::default();
    squashing(&world, true);
    let (done, out) = wizard(
        &setup,
        &world,
        &["--branch", "main", "--source", "cli", "--yes"],
        "",
    );
    assert_eq!(done, Ok(DONE), "{out}");
    for said in [
        "The forge has a merge queue on main, merging by \"squash\".",
        "The forge merges into main through its queue alone, so it refuses herald's own merge.",
        "The repository allows a merge by \"squash\", \"rebase\".",
    ] {
        assert!(out.contains(said), "{said} in {out}");
    }
    let written = config_of(&setup);
    assert!(written.contains("runs = \"forge\"\n"), "{written}");
    assert!(written.contains("method = \"squash\"\n"), "{written}");
    let refused = |flags: &[&str]| {
        let mut all = vec!["--branch", "main", "--source", "cli", "--yes"];
        all.extend(flags);
        wizard(&setup, &world, &all, "").0
    };
    assert_eq!(
        refused(&["--method", "rebase"]),
        Err("--method: answer one of squash".to_owned())
    );
    assert_eq!(
        refused(&["--runs", "herald", "--method", "merge"]),
        Err("--method: answer one of squash, rebase".to_owned())
    );
    squashing(&world, false);
    assert_eq!(
        refused(&["--runs", "herald", "--method", "repository"]),
        Ok(DONE)
    );
    assert!(config_of(&setup).contains("method = \"repository\"\n"));
}
