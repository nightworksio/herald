//! The keys that choose a source's alerts.

use super::*;
use crate::{ALERT_ACTIONS, AlertAction, Alerts, Severity, Tool};

fn alerted(forge: Forge, alerts: Alerts) -> (SourceOptions, Forge) {
    let options = SourceOptions {
        repository: Some("a/b".to_owned()),
        signals: Some(vec![Signal::Alert]),
        alerts: Some(alerts),
        ..SourceOptions::default()
    };
    (options, forge)
}

#[test]
fn alerts_is_given_only_where_alerts_are_announced_and_each_of_its_keys_is_sound() {
    for forge in [Forge::GitHub, Forge::GitLab] {
        let (options, forge) = alerted(forge, Alerts::default());
        assert_eq!(options.check(forge), Ok(()));
    }
    let confirmed = Alerts {
        actions: Some(vec![AlertAction::Confirmed]),
        ..Alerts::default()
    };
    let (options, _) = alerted(Forge::GitLab, confirmed.clone());
    assert_eq!(options.check(Forge::GitLab), Ok(()));
    let unalerted = SourceOptions {
        signals: None,
        ..alerted(Forge::GitHub, Alerts::default()).0
    };
    let refused = [
        (
            unalerted,
            Forge::GitHub,
            "alerts chooses security alerts, which no repository the source takes announces; add \"alert\" to signals, or remove alerts".to_owned(),
        ),
        (
            alerted(Forge::GitHub, Alerts { tools: Some(Vec::new()), ..Alerts::default() }).0,
            Forge::GitHub,
            "alerts.tools names nothing, so no alert would be announced; name one, or remove alerts.tools".to_owned(),
        ),
        (
            alerted(Forge::GitHub, Alerts { actions: Some(Vec::new()), ..Alerts::default() }).0,
            Forge::GitHub,
            "alerts.actions names nothing, so no alert would be announced; name one, or remove alerts.actions".to_owned(),
        ),
        (
            alerted(Forge::GitHub, Alerts { tools: Some(vec![Tool::SecretScanning, Tool::Dast]), ..Alerts::default() }).0,
            Forge::GitHub,
            "alerts.tools \"dast\": github raises no such alerts; its tools are \"code-scanning\", \"dependency-scanning\", \"secret-scanning\"".to_owned(),
        ),
        (
            alerted(Forge::GitHub, confirmed).0,
            Forge::GitHub,
            "alerts.actions \"confirmed\" is GitLab's, and github confirms no alert; remove it".to_owned(),
        ),
    ];
    for (options, forge, said) in refused {
        assert_eq!(options.check(forge), Err(said));
    }
    let forgejo = SourceOptions {
        alerts: None,
        ..alerted(Forge::Forgejo, Alerts::default()).0
    };
    assert_eq!(
        forgejo.check(Forge::Forgejo),
        Err("Forgejo raises no security alerts; remove \"alert\" from signals".to_owned())
    );
    let overridden = SourceOptions {
        repository: None,
        owner: Some("o".to_owned()),
        signals: None,
        repositories: BTreeMap::from([(
            "web".to_owned(),
            Override {
                signals: Some(vec![Signal::Alert]),
                ..Override::default()
            },
        )]),
        ..SourceOptions::default()
    };
    assert!(overridden.check(Forge::Forgejo).is_err());
    assert_eq!(overridden.check(Forge::GitLab), Ok(()));
}

#[test]
fn a_source_announces_every_tool_s_raised_and_reopened_alerts_where_it_chooses_none() {
    let none = SourceOptions::default();
    assert_eq!(none.alert_choice(Forge::GitHub).tools, Tool::GITHUB);
    assert_eq!(none.alert_choice(Forge::GitLab).tools, Tool::ALL);
    assert_eq!(none.alert_choice(Forge::Forgejo).tools, Vec::<Tool>::new());
    assert_eq!(none.alert_choice(Forge::GitHub).actions, ALERT_ACTIONS);
    assert_eq!(ALERT_ACTIONS, [AlertAction::Created, AlertAction::Reopened]);
    let (chosen, _) = alerted(
        Forge::GitLab,
        Alerts {
            tools: Some(vec![Tool::Dast]),
            actions: Some(vec![AlertAction::Fixed]),
            severities: vec![Severity::High, Severity::Critical],
        },
    );
    let choice = chosen.alert_choice(Forge::GitLab);
    assert!(choice.reads(Tool::Dast));
    assert!(!choice.reads(Tool::Fuzzing));
    assert!(choice.takes(Tool::Dast, AlertAction::Fixed, Some(Severity::High)));
    assert!(choice.takes(Tool::Dast, AlertAction::Fixed, Some(Severity::Critical)));
    assert!(!choice.takes(Tool::Dast, AlertAction::Fixed, Some(Severity::Low)));
    assert!(!choice.takes(Tool::Dast, AlertAction::Fixed, None));
    assert!(!choice.takes(Tool::Dast, AlertAction::Created, Some(Severity::High)));
    assert!(!choice.takes(Tool::Other, AlertAction::Fixed, Some(Severity::High)));
    let every = none.alert_choice(Forge::GitLab);
    assert!(every.takes(Tool::Other, AlertAction::Created, None));
    assert!(every.takes(Tool::Other, AlertAction::Reopened, Some(Severity::Note)));
}

#[test]
fn each_tool_action_and_severity_has_the_name_a_route_matches() {
    let tools: Vec<&str> = Tool::ALL.into_iter().map(Tool::name).collect();
    assert_eq!(
        tools,
        [
            "code-scanning",
            "dependency-scanning",
            "secret-scanning",
            "container-scanning",
            "dast",
            "fuzzing",
            "other"
        ]
    );
    let actions: Vec<&str> = [
        AlertAction::Created,
        AlertAction::Reopened,
        AlertAction::Confirmed,
        AlertAction::Fixed,
        AlertAction::Dismissed,
    ]
    .into_iter()
    .map(AlertAction::name)
    .collect();
    assert_eq!(
        actions,
        ["created", "reopened", "confirmed", "fixed", "dismissed"]
    );
    let severities: Vec<&str> = Severity::ALL.into_iter().map(Severity::name).collect();
    assert_eq!(
        severities,
        [
            "critical", "high", "medium", "low", "info", "unknown", "error", "warning", "note"
        ]
    );
    for severity in Severity::ALL {
        assert_eq!(
            Severity::named(&severity.name().to_uppercase()),
            Some(severity)
        );
    }
    assert_eq!(Severity::named("moderate"), Some(Severity::Medium));
    assert_eq!(Severity::named("MODERATE"), Some(Severity::Medium));
    assert_eq!(Severity::named("none"), None);
}
