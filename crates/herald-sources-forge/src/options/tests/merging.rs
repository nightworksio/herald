//! What a forge source gives a merge queue, and how it brings a branch up
//! to date.

use herald_core::merging::{Method, Update, Write};
use herald_kinds::{MergeAsked, Merging, Unmerged};

use super::*;
use crate::options::{
    FORGEJO_QUEUELESS, FORGEJO_RETRIES, GITHUB_APPS, GITHUB_FAST_FORWARD, GITLAB_UPDATES,
};

fn one(repository: &str) -> SourceOptions {
    SourceOptions {
        repository: Some(repository.to_owned()),
        ..SourceOptions::default()
    }
}

fn asking(repository: Option<&str>) -> MergeAsked<'_> {
    MergeAsked {
        repository,
        method: None,
        retries: false,
        write: None,
    }
}

fn refused(key: &'static str, why: &str) -> Result<Merging, Unmerged> {
    Err(Unmerged {
        key,
        why: why.to_owned(),
    })
}

#[test]
fn a_queue_merges_in_the_source_s_one_repository_or_one_it_names_under_the_owner() {
    let merged = one("o/r").merging(Forge::GitHub, &asking(None)).unwrap();
    assert_eq!(
        merged,
        Merging {
            forge: "github",
            repository: "o/r".to_owned(),
            api: "https://api.github.com".to_owned(),
            update: Update::Merge,
            queueless: None,
            reads: crate::merge_read::scopes(Forge::GitHub),
        }
    );
    assert_eq!(
        one("o/r").merging(Forge::GitHub, &asking(Some("o/r"))),
        refused(
            "repository",
            "the source is the one repository o/r; remove repository"
        )
    );
    assert_eq!(
        owner("o").merging(Forge::GitHub, &asking(None)),
        refused(
            "repository",
            "the source is the owner o; name the repository the queue merges in, as owner/name"
        )
    );
    assert_eq!(
        owner("o")
            .merging(Forge::GitHub, &asking(Some("o/r")))
            .map(|merged| merged.repository),
        Ok("o/r".to_owned())
    );
    for elsewhere in ["other/r", "or/r", "o", "o/", "o/a/b"] {
        assert_eq!(
            owner("o").merging(Forge::GitHub, &asking(Some(elsewhere))),
            refused(
                "repository",
                &format!("\"{elsewhere}\" is no repository of the owner o; give one as o/name")
            ),
            "{elsewhere}"
        );
    }
    assert_eq!(
        owner("group/sub")
            .merging(Forge::GitLab, &asking(Some("group/sub/deep/app")))
            .map(|merged| merged.repository),
        Ok("group/sub/deep/app".to_owned())
    );
}

#[test]
fn a_forge_source_answers_a_merge_queue_with_its_options() {
    for forge in [Forge::GitHub, Forge::GitLab, Forge::Forgejo] {
        let source = crate::ForgeSource {
            forge,
            options: one("o/r"),
            names: None,
        };
        let answered = herald_kinds::SourceConfig::merging(&source, &asking(None));
        assert_eq!(answered, Some(one("o/r").merging(forge, &asking(None))));
        assert_eq!(answered.unwrap().unwrap().repository, "o/r");
    }
}

#[test]
fn every_forge_s_own_host_is_where_a_queue_s_steps_go() {
    for (forge, api) in [
        (Forge::GitHub, "https://ghe.example/api/v3"),
        (Forge::GitHub, "https://api.acme.ghe.com"),
        (Forge::GitLab, "https://gitlab.example/api/v4"),
        (Forge::GitLab, "https://acme.gitlab-dedicated.com/api/v4"),
        (Forge::Forgejo, "https://forge.example/api/v1"),
    ] {
        let source = SourceOptions {
            api: Some(format!("{api}/")),
            ..one("o/r")
        };
        assert_eq!(source.merging(forge, &asking(None)).unwrap().api, api);
    }
    assert_eq!(
        one("o/r")
            .merging(Forge::GitLab, &asking(None))
            .unwrap()
            .api,
        "https://gitlab.com/api/v4"
    );
}

#[test]
fn a_method_retries_and_an_app_hold_only_on_a_forge_that_offers_them() {
    let fast = MergeAsked {
        method: Some(Method::FastForward),
        ..asking(None)
    };
    assert_eq!(
        one("o/r").merging(Forge::GitHub, &fast),
        refused("method", GITHUB_FAST_FORWARD)
    );
    assert!(one("o/r").merging(Forge::GitLab, &fast).is_ok());
    assert!(one("o/r").merging(Forge::Forgejo, &fast).is_ok());
    let retried = MergeAsked {
        retries: true,
        ..asking(None)
    };
    assert_eq!(
        one("o/r").merging(Forge::Forgejo, &retried),
        refused("retries", FORGEJO_RETRIES)
    );
    assert!(one("o/r").merging(Forge::GitHub, &retried).is_ok());
    assert!(one("o/r").merging(Forge::GitLab, &retried).is_ok());
    let app = Write {
        token: None,
        app: Some("merger".to_owned()),
        installation: None,
    };
    let as_app = MergeAsked {
        write: Some(&app),
        ..asking(None)
    };
    assert!(one("o/r").merging(Forge::GitHub, &as_app).is_ok());
    for forge in [Forge::GitLab, Forge::Forgejo] {
        assert_eq!(
            one("o/r").merging(forge, &as_app),
            refused("write", GITHUB_APPS)
        );
    }
    let token = Write {
        token: Some(SecretRef {
            secret: "HERALD_MERGE".to_owned(),
        }),
        app: None,
        installation: None,
    };
    let as_token = MergeAsked {
        write: Some(&token),
        ..asking(None)
    };
    assert!(one("o/r").merging(Forge::GitLab, &as_token).is_ok());
}

#[test]
fn only_forgejo_never_has_a_merge_queue_of_its_own() {
    for (forge, queueless) in [
        (Forge::GitHub, None),
        (Forge::GitLab, None),
        (Forge::Forgejo, Some(FORGEJO_QUEUELESS)),
    ] {
        assert_eq!(
            one("o/r").merging(forge, &asking(None)).unwrap().queueless,
            queueless
        );
    }
}

#[test]
fn a_branch_is_merged_up_to_date_except_on_gitlab_which_rebases_alone() {
    for (forge, update, updating) in [
        (Forge::GitHub, None, Update::Merge),
        (Forge::Forgejo, None, Update::Merge),
        (Forge::GitLab, None, Update::Rebase),
        (Forge::GitHub, Some(Update::Rebase), Update::Rebase),
        (Forge::Forgejo, Some(Update::Rebase), Update::Rebase),
        (Forge::GitLab, Some(Update::Rebase), Update::Rebase),
        (Forge::GitHub, Some(Update::Merge), Update::Merge),
    ] {
        let source = SourceOptions {
            update,
            ..one("o/r")
        };
        assert_eq!(source.check(forge), Ok(()));
        assert_eq!(source.updating(forge), updating);
        assert_eq!(
            source.merging(forge, &asking(None)).unwrap().update,
            updating
        );
    }
    let merged = SourceOptions {
        update: Some(Update::Merge),
        ..one("o/r")
    };
    assert_eq!(merged.check(Forge::GitLab), Err(GITLAB_UPDATES.to_owned()));
}

#[test]
fn every_forge_gives_a_writer_for_herald_s_own_queue() {
    let merging = herald_kinds::Merging {
        forge: "github",
        repository: "o/r".to_owned(),
        api: "https://api.github.com".to_owned(),
        update: herald_core::merging::Update::Merge,
        queueless: None,
        reads: "a token",
    };
    for forge in [Forge::GitHub, Forge::GitLab, Forge::Forgejo] {
        let source = crate::ForgeSource {
            forge,
            options: one("o/r"),
            names: None,
        };
        let with = herald_kinds::WriteWith {
            merging: &merging,
            credential: herald_kinds::Credential::Named(herald_ports::Secret::new(
                "HERALD_MERGE",
                "write-value",
            )),
        };
        let writer = herald_kinds::SourceConfig::merge_writer(&source, with);
        assert!(writer.is_some(), "{forge:?}");
    }
}
