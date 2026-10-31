use super::*;
use crate::event::tests::{draft, event};
use crate::event::{Draft, Event};

const TITLE: &str =
    "📦 {{ event.project }} {{ event.version }}{% if event.prerelease %} · pre-release{% endif %}";

fn templates(sources: &[(&str, &str)]) -> Templates {
    let lines: Vec<(&str, &str, Form)> = sources
        .iter()
        .map(|&(name, source)| (name, source, Form::Line))
        .collect();
    Templates::compile(&lines).unwrap()
}

fn markdown(source: &str, event: &Event) -> String {
    let route = RouteId::new("r");
    Templates::compile(&[("t", source, Form::Markdown)])
        .unwrap()
        .render(
            "t",
            &Context {
                event,
                route: &route,
                herald: "0.1.0",
            },
        )
        .unwrap()
}

fn render(source: &str, event: &Event) -> Result<String, TemplateError> {
    let route = RouteId::new("php-mutation-gate-releases");
    templates(&[("t", source)]).render(
        "t",
        &Context {
            event,
            route: &route,
            herald: "0.1.0",
        },
    )
}

#[test]
fn a_template_renders_the_event_route_and_herald() {
    assert_eq!(
        render(TITLE, &event()).unwrap(),
        "📦 php-mutation-gate v0.1.0"
    );
    assert_eq!(
        render(
            "{{ route }} by herald {{ herald }}: {{ event.links.release }}",
            &event()
        )
        .unwrap(),
        "php-mutation-gate-releases by herald 0.1.0: https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0"
    );
}

#[test]
fn a_prerelease_is_marked_where_the_template_asks() {
    let prerelease = Event::normalise(Draft {
        prerelease: true,
        ..draft()
    })
    .unwrap();
    assert_eq!(
        render(TITLE, &prerelease).unwrap(),
        "📦 php-mutation-gate v0.1.0 · pre-release"
    );
}

#[test]
fn an_undefined_value_may_be_tested_but_not_printed() {
    assert_eq!(
        render(
            "{% if event.links.changelog %}yes{% else %}no{% endif %}",
            &event()
        )
        .unwrap(),
        "no"
    );
    let error = render("{{ event.release_notes }}", &event()).unwrap_err();
    assert_eq!(error.name, "t");
    assert!(error.reason.contains("undefined"), "{}", error.reason);
}

#[test]
fn a_render_line_is_on_one_line() {
    let route = RouteId::new("r");
    let line = templates(&[("t", "  {{ event.project }}\n\n  {{ event.version }} ")])
        .render_line(
            "t",
            &Context {
                event: &event(),
                route: &route,
                herald: "0.1.0",
            },
        )
        .unwrap();
    assert_eq!(line, "php-mutation-gate v0.1.0");
    let error = templates(&[])
        .render_line(
            "missing",
            &Context {
                event: &event(),
                route: &route,
                herald: "0.1.0",
            },
        )
        .unwrap_err();
    assert_eq!(error.name, "missing");
}

#[test]
fn every_template_that_does_not_compile_is_named() {
    let errors = Templates::compile(&[
        ("good", "{{ event.title }}", Form::Line),
        ("bad", "{% if %}", Form::Markdown),
        ("worse", "{{", Form::Line),
    ])
    .unwrap_err();
    let names: Vec<&str> = errors.iter().map(|error| error.name.as_str()).collect();
    assert_eq!(names, ["bad", "worse"]);
    assert!(
        errors[0].to_string().starts_with("template bad: "),
        "{}",
        errors[0]
    );
}

#[test]
fn a_template_that_does_not_exist_is_an_error() {
    let route = RouteId::new("r");
    let error = templates(&[])
        .render(
            "missing",
            &Context {
                event: &event(),
                route: &route,
                herald: "0.1.0",
            },
        )
        .unwrap_err();
    assert_eq!(error.name, "missing");
}

#[test]
fn a_template_includes_another_by_name_and_nothing_from_disk() {
    let route = RouteId::new("r");
    let context = Context {
        event: &event(),
        route: &route,
        herald: "0.1.0",
    };
    let set = templates(&[
        ("footer", "— {{ event.project }}"),
        ("body", "{{ event.title }} {% include 'footer' %}"),
    ]);
    assert_eq!(
        set.render("body", &context).unwrap(),
        "php-mutation-gate v0.1.0 — php-mutation-gate"
    );
    let set = templates(&[("body", "{% include '/etc/passwd' %}")]);
    assert!(set.render("body", &context).is_err());
}

#[test]
fn a_template_that_runs_too_long_runs_out_of_fuel() {
    let error = render(
        "{% for a in range(1000) %}{% for b in range(1000) %}{% endfor %}{% endfor %}",
        &event(),
    )
    .unwrap_err();
    assert_eq!(error.reason, "it runs too long: it ran out of fuel");
}

#[test]
fn an_output_past_the_cap_is_refused() {
    let at_cap = format!("{{{{ ' ' * {OUTPUT_CAP} }}}}");
    assert_eq!(
        render(&at_cap, &event()).map(|output| output.len()),
        Ok(OUTPUT_CAP)
    );
    let past_cap = format!("{{{{ ' ' * {} }}}}", OUTPUT_CAP + 1);
    let error = render(&past_cap, &event()).unwrap_err();
    assert_eq!(
        error.reason,
        format!("its output passes the cap of {OUTPUT_CAP} bytes")
    );
}

#[test]
fn markup_in_event_text_is_left_to_the_dialect_conversion() {
    let html = Event::normalise(Draft {
        title: "<b>x</b>".to_owned(),
        ..draft()
    })
    .unwrap();
    assert_eq!(render("{{ event.title }}", &html).unwrap(), "<b>x</b>");
}

#[test]
fn a_markdown_template_places_event_text_literally_and_the_body_as_markdown() {
    let hostile = Event::normalise(Draft {
        title: "[click](https://evil.example) *now* @everyone <@&1> #1".to_owned(),
        body: "**bold** [docs](https://example.org)".to_owned(),
        ..draft()
    })
    .unwrap();
    assert_eq!(
        markdown("## {{ event.title }}\n\n{{ event.body }}", &hostile),
        "## \\[click\\]\\(https\\:\\/\\/evil\\.example\\) \\*now\\* \\@everyone \\<\\@\\&1\\> \\#1\n\n**bold** [docs](https://example.org)"
    );
}

#[test]
fn a_markdown_template_prints_numbers_and_absent_values_as_a_line_template_does() {
    let unversioned = Event::normalise(Draft {
        version: None,
        ..draft()
    })
    .unwrap();
    assert_eq!(
        markdown("{{ 1 + 1 }} {{ event.version }}", &unversioned),
        render("{{ 1 + 1 }} {{ event.version }}", &unversioned).unwrap()
    );
    assert!(
        Templates::compile(&[("t", "{{ event.nope }}", Form::Markdown)])
            .unwrap()
            .render(
                "t",
                &Context {
                    event: &event(),
                    route: &RouteId::new("r"),
                    herald: "0.1.0"
                }
            )
            .is_err()
    );
}
