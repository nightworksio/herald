use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use herald_config::{Level, LogFormat};
use herald_ports::{Scrubber, Secret};

use super::*;

fn configured(level: Level, format: LogFormat) -> herald_config::Log {
    herald_config::Log {
        level,
        format,
        ..herald_config::Log::default()
    }
}

fn scrubber() -> Scrubber {
    Scrubber::new(herald_config::Log::SCRUB_FROM)
}

fn told() -> [Told; 3] {
    [
        Told::info("polled"),
        Told::warn("a request was refused"),
        Told::error("stopped"),
    ]
}

#[test]
fn a_log_writes_each_line_at_least_as_grave_as_its_level() {
    let none = |_: &str| None;
    let lines = |level| {
        Log::of(&configured(level, LogFormat::Text), none, scrubber()).map(|log| log.lines(told()))
    };
    assert_eq!(
        lines(Level::Info),
        Ok(vec![
            "polled".to_owned(),
            "a request was refused".to_owned(),
            "stopped".to_owned()
        ])
    );
    assert_eq!(
        lines(Level::Warn),
        Ok(vec![
            "a request was refused".to_owned(),
            "stopped".to_owned()
        ])
    );
    assert_eq!(lines(Level::Error), Ok(vec!["stopped".to_owned()]));
}

#[test]
fn a_json_log_writes_an_object_a_line_with_its_level_and_its_scrubbed_message() {
    let scrubber = scrubber();
    let token = format!("ghs_{}", "q7Rt".repeat(9));
    scrubber.keep(&Secret::new("github_apps.release", token.as_str()));
    let log = Log::of(
        &configured(Level::Info, LogFormat::Json),
        |_| None,
        scrubber,
    );
    let written = log.map(|log| {
        log.lines([
            Told::error(format!("the forge said \"{token}\" is unknown")),
            Told::info("done"),
        ])
    });
    assert_eq!(
        written,
        Ok(vec![
            r#"{"level":"error","message":"the forge said \"«secret:github_apps.release»\" is unknown"}"#
                .to_owned(),
            r#"{"level":"info","message":"done"}"#.to_owned(),
        ])
    );
}

#[test]
fn the_variables_take_the_config_s_place_and_the_flags_take_theirs() {
    let vars = BTreeMap::from([(LOG_LEVEL, "error"), (LOG_FORMAT, "json")]);
    let var = |name: &str| vars.get(name).copied();
    let log = Log::of(&configured(Level::Info, LogFormat::Text), var, scrubber());
    let Ok(log) = log else {
        panic!("{log:?}");
    };
    assert_eq!(log.format(), LogFormat::Json);
    assert_eq!(
        log.lines(told()),
        vec![r#"{"level":"error","message":"stopped"}"#.to_owned()]
    );
    let flagged = log
        .clone()
        .flagged(Some(Level::Warn), Some(LogFormat::Text));
    assert_eq!(flagged.format(), LogFormat::Text);
    assert_eq!(
        flagged.lines(told()),
        vec!["a request was refused", "stopped"]
    );
    let unflagged = log.flagged(None, None);
    assert_eq!(unflagged.format(), LogFormat::Json);
    assert_eq!(unflagged.lines(told()).len(), 1);
}

#[test]
fn a_variable_naming_no_level_or_format_is_a_mistake_that_says_what_it_takes() {
    let config = herald_config::Log::default();
    let loud = BTreeMap::from([(LOG_LEVEL, "loud")]);
    assert_eq!(
        Log::of(&config, |name| loud.get(name).copied(), scrubber()).map(|_| ()),
        Err("HERALD_LOG_LEVEL is \"loud\"; it takes info, warn, error".to_owned())
    );
    let yaml = BTreeMap::from([(LOG_FORMAT, "yaml")]);
    assert_eq!(
        Log::of(&config, |name| yaml.get(name).copied(), scrubber()).map(|_| ()),
        Err("HERALD_LOG_FORMAT is \"yaml\"; it takes text, json".to_owned())
    );
    assert_eq!(level("warn"), Ok(Level::Warn));
    assert_eq!(format("text"), Ok(LogFormat::Text));
}

#[test]
fn a_line_keeps_its_level_as_it_is_reworded_and_reads_as_its_words() {
    let told = Told::warn("refused").reworded(|text| format!("source: {text}"));
    assert_eq!(told, Told::at(Level::Warn, "source: refused"));
    assert_eq!(told, "source: refused");
    assert_ne!(told, "refused");
}

#[test]
fn the_scrubber_a_log_holds_is_the_one_it_was_given() {
    let given = Scrubber::new(NonZeroUsize::MIN);
    let log = Log::of(&herald_config::Log::default(), |_| None, given.clone());
    given.keep(&Secret::new("ONE", "x"));
    assert_eq!(
        log.map(|log| log.scrubber().scrubbed("x")),
        Ok("«secret:ONE»".to_owned())
    );
}

#[test]
fn a_log_before_any_config_takes_the_level_and_format_the_variables_name_and_else_the_defaults() {
    let vars = BTreeMap::from([(LOG_LEVEL, "error"), (LOG_FORMAT, "json")]);
    let named = Log::unconfigured(|name| vars.get(name).copied());
    assert_eq!(named.format(), LogFormat::Json);
    assert_eq!(
        named.lines(told()),
        vec![r#"{"level":"error","message":"stopped"}"#.to_owned()]
    );
    let wrong = BTreeMap::from([(LOG_LEVEL, "loud"), (LOG_FORMAT, "yaml")]);
    let defaults = Log::unconfigured(|name| wrong.get(name).copied());
    assert_eq!(defaults.format(), LogFormat::Text);
    assert_eq!(defaults.lines(told()).len(), 3);
}
