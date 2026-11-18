//! Each forge's run events, read from what each forge sends, as a source
//! announcing runs takes them.

#[cfg(test)]
mod runs {
    use herald_core::{Attribute, Kind, Signal};
    use herald_kinds::{Hooked, SourceConfig, SourceKind};
    use herald_sources_forge::{
        ForgeSource, ForgejoKind, GitHubKind, GitLabKind, SourceOptions, ci_run, of_a_run,
    };
    use proptest::prelude::*;

    const GITHUB: &str = include_str!("../fixtures/github/workflow-run-event.reference.json");
    const GITLAB: &str = include_str!("../fixtures/gitlab/pipeline-hook.reference.json");
    const FORGEJO: &str = include_str!("../fixtures/forgejo/workflow-run-event.reference.json");

    fn options(repository: &str) -> SourceOptions {
        SourceOptions {
            repository: Some(repository.to_owned()),
            signals: Some(vec![Signal::Run]),
            ..SourceOptions::default()
        }
    }

    fn text(attributes: &std::collections::BTreeMap<String, Attribute>, name: &str) -> String {
        match &attributes[name] {
            Attribute::Text(text) => text.clone(),
            other => format!("{other:?}"),
        }
    }

    fn announced(source: &ForgeSource, header: (&str, &str), payload: &str) -> herald_core::Event {
        match source.hooked(&[header], payload.as_bytes()).unwrap() {
            Hooked::Event(event) => event,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_github_workflow_run_event_is_announced_as_its_run_from_a_webhook_or_the_run() {
        let options = options("cli/cli");
        let source = GitHubKind::checked(options.clone()).unwrap();
        assert_eq!(source.kinds(), [Kind::RUN]);
        let event = announced(&source, ("X-GitHub-Event", "workflow_run"), GITHUB);
        assert_eq!(
            event.identity().as_str(),
            "github.com/212613049/run/30433642/1"
        );
        assert_eq!(event.title(), "Build failed on main");
        assert_eq!(event.body(), "Update README.md");
        assert_eq!(text(event.attributes(), "trigger"), "push");
        assert_eq!(text(event.attributes(), "actor"), "octocat");
        assert_eq!(event.origin().unwrap().topics, ["cli", "github"]);
        assert!(of_a_run(
            herald_sources_forge::Forge::GitHub,
            "workflow_run"
        ));
        let ran = ci_run(
            (herald_sources_forge::Forge::GitHub, &options),
            ("workflow_run", GITHUB.as_bytes()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ran.identity(), event.identity());
    }

    #[test]
    fn a_gitlab_pipeline_hook_is_announced_as_its_run() {
        let source = GitLabKind::checked(SourceOptions {
            api: Some("https://gitlab.example.com/api/v4".to_owned()),
            ..options("gitlab-org/gitlab-test")
        })
        .unwrap();
        let event = announced(&source, ("X-Gitlab-Event", "Pipeline Hook"), GITLAB);
        assert_eq!(
            event.identity().as_str(),
            "gitlab.example.com/1/run/31/success"
        );
        assert_eq!(event.title(), "Pipeline for branch: main succeeded on main");
        assert_eq!(text(event.attributes(), "trigger"), "pull_request");
        assert_eq!(text(event.attributes(), "actor"), "root");
        assert_eq!(
            event.link("run"),
            Some("https://gitlab.example.com/gitlab-org/gitlab-test/-/pipelines/31")
        );
    }

    #[test]
    fn a_forgejo_run_event_is_announced_as_its_run() {
        let source = ForgejoKind::checked(options("forgejo/forgejo")).unwrap();
        let event = announced(
            &source,
            ("X-Forgejo-Event", "workflow_run_failure"),
            FORGEJO,
        );
        assert_eq!(
            event.identity().as_str(),
            "codeberg.org/76/run/30433/failure"
        );
        assert_eq!(event.title(), "testing.yml failed on forgejo");
        assert_eq!(event.occurred().to_string(), "2026-11-17T10:02:47Z");
        let older = announced(&source, ("X-Gitea-Event", "action_run_failure"), FORGEJO);
        assert_eq!(older.identity(), event.identity());
    }

    proptest! {
        #[test]
        fn no_run_delivery_however_garbled_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let sources = [
                (GitHubKind::checked(options("cli/cli")).unwrap(), ("X-GitHub-Event", "workflow_run")),
                (GitHubKind::checked(options("cli/cli")).unwrap(), ("X-GitHub-Event", "check_suite")),
                (GitHubKind::checked(options("cli/cli")).unwrap(), ("X-GitHub-Event", "status")),
                (GitLabKind::checked(options("a/b")).unwrap(), ("X-Gitlab-Event", "Pipeline Hook")),
                (ForgejoKind::checked(options("a/b")).unwrap(), ("X-Forgejo-Event", "workflow_run_success")),
            ];
            for (source, header) in sources {
                let _ = source.hooked(&[header], &bytes);
            }
        }
    }
}
