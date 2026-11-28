use super::world::*;
use crate::*;

fn previewed(config: &str, args: &[&str]) -> Run {
    let mut all = vec!["preview"];
    all.extend(args);
    herald(&setup(config), &World::default(), &all)
}

#[test]
fn preview_wire_prints_an_email_sent_through_mailgun_to_the_addresses_the_config_writes() {
    let config = format!(
        "{CONFIG}{}",
        r#"[destinations.mail]
kind = "email"
transport = "mailgun"
from = "Releases <releases@mg.example.org>"
region = "eu"
domain = "mg.example.org"
api_key = { secret = "MAILGUN_API_KEY" }
[[routes]]
id = "mailed"
sources = ["cli"]
destination = "mail"
template = "release"
to = "Team <team@example.org>"
bcc = ["hidden@example.org"]
"#
    );
    let shown = previewed(&config, &["--format", "wire", "--sample", "plain"]);
    assert_eq!(
        (shown.code, shown.err.as_str()),
        (DONE, ""),
        "{}",
        shown.out
    );
    let mailed: Vec<&str> = shown
        .out
        .lines()
        .skip_while(|line| !line.starts_with("mailed to mail"))
        .filter(|line| !line.starts_with("    User-Agent: "))
        .collect();
    assert_eq!(
        mailed.get(..5).unwrap_or_else(|| panic!("{}", shown.out)),
        [
            "mailed to mail, sample release (plain):",
            "  post:",
            "    POST https://api.eu.mailgun.net/v3/mg.example.org/messages.mime",
            "    Authorization: Basic «secret:MAILGUN_API_KEY»",
            "    Content-Type: multipart/form-data; boundary=\"=_herald_form\"",
        ]
    );
    let form = mailed.join("\n");
    for field in [
        "Content-Disposition: form-data; name=\"to\"\n    \n    team@example.org\n",
        "Content-Disposition: form-data; name=\"to\"\n    \n    hidden@example.org\n",
        "\n    To: \"Team\" <team@example.org>\n",
        "\n    From: \"Releases\" <releases@mg.example.org>\n",
    ] {
        assert!(form.contains(field), "{field}\n{form}");
    }
    assert!(!form.contains("Bcc"), "{form}");
}
