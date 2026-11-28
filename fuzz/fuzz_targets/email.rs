//! Whatever text an announcement carries and whatever a config names, the
//! email message herald writes reads back to exactly the fields, recipients
//! and parts herald meant: no text adds a field, a recipient or a part, a
//! blind copy stands in no field, and an address herald takes holds nothing
//! but what an address may.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../crates/herald-dest-email/tests/email/read.rs"]
mod read;

#[path = "../../crates/herald-dest-email/tests/email/judge.rs"]
mod judge;

fuzz_target!(|bytes: &[u8]| {
    let Some((&flags, rest)) = bytes.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let mut pieces = text.split('\u{1f}');
    let mut next = || pieces.next().unwrap_or_default();
    let (subject, body, html, sender, recipient, address) =
        (next(), next(), next(), next(), next(), next());
    judge::judge(&judge::Given {
        subject,
        text: body,
        html: (flags & 1 == 1).then_some(html),
        sender,
        recipient,
        address,
        replying: flags & 2 == 2,
    });
});
