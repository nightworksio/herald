use proptest::prelude::*;

use crate::judge::{Given, judge};

/// Text weighted toward what breaks a message: line breaks, folding
/// whitespace, quotes, brackets, the encoded-word and quoted-printable
/// delimiters, the boundary's characters, and any other character.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("\r\n".to_owned()),
        Just("\r".to_owned()),
        Just("\n".to_owned()),
        Just(" ".to_owned()),
        Just("\t".to_owned()),
        Just("=?".to_owned()),
        Just("?=".to_owned()),
        Just("=_".to_owned()),
        Just("--".to_owned()),
        Just("\"".to_owned()),
        Just("<>,@".to_owned()),
        Just("é🚀".to_owned()),
        "[a-zA-Z0-9.]{1,90}",
        any::<char>().prop_map(String::from),
    ];
    proptest::collection::vec(piece, 0..40).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn any_text_reads_back_as_written_and_adds_no_field_recipient_or_part(
        subject in text(),
        body in text(),
        html in proptest::option::of(text()),
        sender in text(),
        recipient in text(),
        address in prop_oneof!["[a-z0-9.!#$%&'*+/=?^_`{|}~-]{1,20}@[a-z0-9-]{1,10}(\\.[a-z0-9-]{1,10}){0,2}", text()],
        replying in any::<bool>(),
    ) {
        judge(&Given {
            subject: &subject,
            text: &body,
            html: html.as_deref(),
            sender: &sender,
            recipient: &recipient,
            address: &address,
            replying,
        });
    }
}
