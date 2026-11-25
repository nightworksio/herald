use super::*;

/// The flags every refusal below starts from.
const FLAGS: [&str; 7] = [
    "--branch", "main", "--method", "squash", "--runs", "herald", "--yes",
];

/// Why the wizard on `config` stopped, with [`FLAGS`] and each of `given`
/// in place of the same flag's value or added; the config left as it was.
fn refused(config: &str, given: &[(&str, &str)]) -> String {
    let setup = setup(config);
    let world = World::default();
    ruled(&world, false);
    let mut flags = FLAGS.to_vec();
    for (flag, value) in given {
        match flags.iter().position(|named| named == flag) {
            Some(at) => flags[at + 1] = value,
            None => flags.extend([*flag, *value]),
        }
    }
    let (done, out) = wizard(&setup, &world, &flags, "");
    assert_eq!(config_of(&setup), config, "{out}");
    done.unwrap_err()
}

#[test]
fn each_flag_s_refused_value_stops_the_wizard_naming_the_flag() {
    let apps = format!("{CONFIG}{MERGER}");
    for (config, given, why) in [
        (
            CONFIG,
            &[("--source", "nowhere")][..],
            "--source: answer one of cli",
        ),
        (
            CONFIG,
            &[("--branch", " ")],
            "--branch: give the branch's name, such as main",
        ),
        (CONFIG, &[("--label", " ")], "--label: give it, not blank"),
        (
            CONFIG,
            &[("--retry", "build"), ("--retry-times", "0")],
            "--retry-times: answer a number from 1 to 255",
        ),
        (
            apps.as_str(),
            &[("--write-app", "nobody")],
            "--write-app: answer one of merger",
        ),
        (
            apps.as_str(),
            &[("--write-app", "merger"), ("--installation", "many")],
            "--installation: answer the installation's id, a number, or none",
        ),
    ] {
        assert_eq!(refused(config, given), why);
    }
    let why = refused(CONFIG, &[("--repository", "cli/other")]);
    assert!(
        why.starts_with("--repository: ")
            && why.contains("the source is the one repository cli/cli"),
        "{why}"
    );
}

#[test]
fn herald_s_own_keys_are_refused_on_a_queue_the_forge_runs() {
    for (flag, value) in [
        ("--may-fail", "lint"),
        ("--wait", "1h"),
        ("--retry", "build"),
        ("--retry-times", "2"),
    ] {
        assert_eq!(
            refused(CONFIG, &[("--runs", "forge"), (flag, value)]),
            "--may-fail, --wait, --retry and --retry-times hold for herald's own queue alone; give --runs herald, or leave them out"
        );
    }
}

#[test]
fn a_table_the_config_refuses_is_said_and_never_written() {
    let why = refused(CONFIG, &[("--may-fail", " ")]);
    assert!(
        why.contains("a check is named as the forge shows it, not blank"),
        "{why}"
    );
    assert!(why.ends_with("\nNothing was written."), "{why}");
}

#[test]
fn a_question_refused_the_most_times_stops_the_wizard_and_one_fewer_does_not() {
    let most = usize::from(crate::queue_setup::MOST_REFUSED);
    let answered = |refusals: usize| {
        let setup = setup(CONFIG);
        let world = World::default();
        ruled(&world, false);
        let answers = format!(
            "\nmain\n{}{}squash\n{}y\n",
            "\n".repeat(7),
            "fast-forward\n".repeat(refusals),
            "\n".repeat(6)
        );
        let (done, out) = wizard(&setup, &world, &[], &answers);
        (done, out, config_of(&setup))
    };
    let (done, out, written) = answered(most - 1);
    assert_eq!(done, Ok(DONE), "{out}");
    assert!(written.contains("method = \"squash\""), "{written}");
    let (done, out, written) = answered(most);
    assert_eq!(
        done,
        Err(format!(
            "no answer to \"How it merges\" was taken in {most} tries; nothing was written"
        )),
        "{out}"
    );
    assert_eq!(out.matches("How it merges: ").count(), most, "{out}");
    assert_eq!(written, CONFIG);
}
