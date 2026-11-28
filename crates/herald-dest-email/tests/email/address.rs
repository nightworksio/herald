use herald_dest_email::{
    ADDRESS_MOST, AddrSpec, LABEL_MOST, LOCAL_MOST, Mailbox, NAME_MOST, Unparsed,
};

fn address(text: &str) -> AddrSpec {
    AddrSpec::parse(text).expect("an address")
}

#[test]
fn a_mailbox_is_written_as_an_address_alone_or_with_its_name_quoted_or_not() {
    let plain = Mailbox::parse("herald@example.org").expect("an address");
    assert_eq!(plain.name(), None);
    assert_eq!(plain.address().local(), "herald");
    assert_eq!(plain.address().domain(), "example.org");
    assert_eq!(plain.address().to_string(), "herald@example.org");

    for text in [
        "Release notes <notes@example.org>",
        "  \"Release notes\"  <notes@example.org>  ",
        "\"  Release notes \" <notes@example.org>",
    ] {
        let named = Mailbox::parse(text).expect("a named address");
        assert_eq!(named.name(), Some("Release notes"), "{text}");
        assert_eq!(named.address(), &address("notes@example.org"));
    }
    for text in [
        "<notes@example.org>",
        " \"\" <notes@example.org>",
        "   <notes@example.org>",
    ] {
        assert_eq!(
            Mailbox::parse(text).expect("an address").name(),
            None,
            "{text}"
        );
    }
    let unicode = Mailbox::parse("Zoë's bot ✨ <z@example.org>").expect("a named address");
    assert_eq!(unicode.name(), Some("Zoë's bot ✨"));
    assert_eq!(
        Mailbox::new(Some("   "), address("a@example.org"))
            .expect("no name")
            .name(),
        None
    );
}

#[test]
fn every_atom_character_and_the_longest_parts_are_taken() {
    let atoms = "!#$%&'*+-/=?^_`{|}~.Az09";
    assert_eq!(address(&format!("{atoms}@a-1.example")).local(), atoms);
    let local = "l".repeat(LOCAL_MOST);
    let label = "d".repeat(LABEL_MOST);
    assert_eq!(address(&format!("{local}@{label}")).local(), local);
    let last = "e".repeat(ADDRESS_MOST - LOCAL_MOST - 1 - 2 * (LABEL_MOST + 1));
    let longest = format!("{local}@{label}.{label}.{last}");
    assert_eq!(longest.len(), ADDRESS_MOST);
    assert_eq!(address(&longest).to_string(), longest);
    assert_eq!(address("a@localhost").domain(), "localhost");
}

#[test]
fn an_address_is_refused_for_what_it_holds_and_the_reason_repeats_none_of_it() {
    let cases: &[(&str, Unparsed)] = &[
        ("", Unparsed::Empty),
        ("no-at-sign", Unparsed::Unaddressed),
        ("a@b@example.org", Unparsed::Local),
        (".a@example.org", Unparsed::Local),
        ("a.@example.org", Unparsed::Local),
        ("a..b@example.org", Unparsed::Local),
        ("@example.org", Unparsed::Local),
        ("\"quoted\"@example.org", Unparsed::Local),
        ("a b@example.org", Unparsed::Local),
        ("jö@example.org", Unparsed::Local),
        ("a@", Unparsed::Domain),
        ("a@-example.org", Unparsed::Domain),
        ("a@example-.org", Unparsed::Domain),
        ("a@example..org", Unparsed::Domain),
        ("a@example.org.", Unparsed::Domain),
        ("a@exa_mple.org", Unparsed::Domain),
        ("a@[192.0.2.1]", Unparsed::Domain),
        ("a@exämple.org", Unparsed::Domain),
        ("a@example.org\r\n", Unparsed::Domain),
        ("a@example.org\r\nBcc: victim@example.org", Unparsed::Local),
    ];
    for &(text, why) in cases {
        assert_eq!(AddrSpec::parse(text), Err(why), "{text:?}");
    }
    let local = format!("{}@example.org", "l".repeat(LOCAL_MOST + 1));
    assert_eq!(AddrSpec::parse(&local), Err(Unparsed::Local));
    let wide = format!("a@{}.org", "d".repeat(LABEL_MOST + 1));
    assert_eq!(AddrSpec::parse(&wide), Err(Unparsed::Domain));
    let label = "d".repeat(LABEL_MOST);
    let long = format!(
        "a@{label}.{label}.{label}.{}",
        "d".repeat(ADDRESS_MOST - 2 - 3 * (LABEL_MOST + 1) + 1)
    );
    assert_eq!(long.len(), ADDRESS_MOST + 1);
    assert_eq!(AddrSpec::parse(&long), Err(Unparsed::Long));
}

#[test]
fn a_mailbox_is_refused_for_a_control_character_stray_brackets_or_a_name_it_cannot_show() {
    let cases: &[(&str, Unparsed)] = &[
        ("", Unparsed::Empty),
        ("   ", Unparsed::Empty),
        ("a@example.org\r\n", Unparsed::Control),
        ("Name\n <a@example.org>", Unparsed::Control),
        ("a@example.org\u{85}", Unparsed::Control),
        ("Name <a@example.org> after", Unparsed::Unaddressed),
        ("Name a@example.org", Unparsed::Unaddressed),
        ("a@example.org>", Unparsed::Unaddressed),
        ("<a@example.org", Unparsed::Unaddressed),
        ("a@example.org, b@example.org", Unparsed::Unaddressed),
        ("Name <a b@example.org>", Unparsed::Local),
        ("\"Na\"me\" <a@example.org>", Unparsed::Name),
        ("Na\\me <a@example.org>", Unparsed::Name),
        ("Na<me <a@example.org>", Unparsed::Name),
        ("Na>me <a@example.org>", Unparsed::Name),
        ("Line\u{2028}break <a@example.org>", Unparsed::Name),
        ("Para\u{2029}graph <a@example.org>", Unparsed::Name),
        ("\" <a@example.org>", Unparsed::Name),
    ];
    for &(text, why) in cases {
        assert_eq!(Mailbox::parse(text), Err(why), "{text:?}");
    }
    let at = address("a@example.org");
    assert_eq!(
        Mailbox::new(Some("x\r\nBcc: v@example.org"), at.clone()),
        Err(Unparsed::Name)
    );
    assert_eq!(
        Mailbox::new(Some("tab\there"), at.clone()),
        Err(Unparsed::Name)
    );
    let longest = "n".repeat(NAME_MOST);
    assert_eq!(
        Mailbox::new(Some(&longest), at.clone())
            .expect("a name")
            .name(),
        Some(longest.as_str())
    );
    let longer = "n".repeat(NAME_MOST + 1);
    assert_eq!(Mailbox::new(Some(&longer), at), Err(Unparsed::Name));
}

#[test]
fn a_reason_names_what_herald_takes_and_never_the_text() {
    let reasons = [
        (Unparsed::Empty, "is empty"),
        (
            Unparsed::Control,
            "holds a control character, such as a line break",
        ),
        (
            Unparsed::Unaddressed,
            "is no address: write it as local@domain or as Name <local@domain>",
        ),
        (
            Unparsed::Local,
            "has a local part herald does not write: letters, digits and !#$%&'*+-/=?^_`{|}~ in runs joined by single dots, at most 64 bytes",
        ),
        (
            Unparsed::Domain,
            "has a domain herald does not write: labels of ASCII letters, digits and hyphens joined by single dots, each at most 63 bytes and starting and ending with a letter or digit; write an international domain in its xn-- form",
        ),
        (
            Unparsed::Long,
            "is longer than the 254 bytes an address holds",
        ),
        (
            Unparsed::Name,
            "has a display name herald does not write: at most 256 bytes, with no control character, line separator, quote, backslash, < or >",
        ),
    ];
    for (why, said) in reasons {
        assert_eq!(why.to_string(), said);
    }
    let typed = format!("{}-{}", "typed", 7 * 6);
    let refused: Result<Mailbox, _> =
        serde_json::from_value(serde_json::json!(format!("{typed} <x>")));
    let error = refused.expect_err("refused").to_string();
    assert!(error.starts_with("the address is no address"), "{error}");
    assert!(!error.contains(&typed), "{error}");
}

#[test]
fn a_config_writes_a_mailbox_as_one_string_described_by_its_schema() {
    let read: Mailbox =
        serde_json::from_value(serde_json::json!("Bot <bot@example.org>")).expect("a mailbox");
    assert_eq!(
        read,
        Mailbox::parse("Bot <bot@example.org>").expect("a mailbox")
    );
    let schema = serde_json::to_value(schemars::schema_for!(Mailbox)).expect("a schema");
    assert_eq!(schema["title"], "Mailbox");
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["minLength"], 3);
    assert_eq!(
        schema["description"],
        "An address, written local@domain or Name <local@domain>."
    );
}

#[test]
fn an_address_is_the_same_mailbox_in_any_case_of_its_domain_alone() {
    let address = address("Herald@Example.org");
    assert!(address.same(&AddrSpec::parse("Herald@example.ORG").expect("an address")));
    assert!(!address.same(&AddrSpec::parse("herald@example.org").expect("an address")));
    assert!(!address.same(&AddrSpec::parse("Herald@example.net").expect("an address")));
}
