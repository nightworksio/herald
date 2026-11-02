use std::borrow::Cow;

/// The longest entity name read; a longer run after `&` is text.
const LONGEST: usize = 10;

/// The character a named or numeric reference, without its `&` and `;`,
/// stands for: XML's five, the HTML names feeds use most, and any
/// character by its number. Anything else is not known.
pub(crate) fn resolved(name: &str) -> Option<Cow<'static, str>> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse::<u32>().ok()?,
        };
        let character = char::from_u32(code)
            .filter(|character| *character != '\0')
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        return Some(Cow::Owned(character.to_string()));
    }
    let character = match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "bull" => "•",
        "middot" => "·",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "times" => "×",
        _ => return None,
    };
    Some(Cow::Borrowed(character))
}

/// `text` with every reference it knows replaced by its character, and
/// every other `&` left as it is.
pub(crate) fn decoded(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let known = after
            .split_once(';')
            .filter(|(name, _)| name.len() <= LONGEST)
            .and_then(|(name, tail)| resolved(name).map(|character| (character, tail)));
        if let Some((character, tail)) = known {
            out.push_str(&character);
            rest = tail;
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn known_references_resolve_and_the_rest_stay_text() {
        assert_eq!(
            decoded("a &amp; b &lt;c&gt; &quot;d&apos; &#65;&#x42;&#X43; &hellip;"),
            "a & b <c> \"d' ABC …"
        );
        assert_eq!(decoded("&unknown; & &; &amp"), "&unknown; & &; &amp");
        assert_eq!(
            decoded("&#0; &#xD800; &#99999999;"),
            "\u{fffd} \u{fffd} \u{fffd}"
        );
        assert_eq!(
            decoded("&averyveryverylongname;"),
            "&averyveryverylongname;"
        );
        assert_eq!(decoded("&#000000065; &#0000000065;"), "A &#0000000065;");
        assert!(matches!(decoded("plain"), Cow::Borrowed("plain")));
        for name in [
            "nbsp", "mdash", "ndash", "lsquo", "rsquo", "ldquo", "rdquo", "laquo", "raquo", "bull",
            "middot", "copy", "reg", "trade", "times",
        ] {
            assert!(resolved(name).is_some(), "{name}");
        }
        assert_eq!(resolved("#x"), None);
    }

    proptest! {
        #[test]
        fn text_without_an_ampersand_is_unchanged(text in "[^&]{0,40}") {
            prop_assert_eq!(decoded(&text), text.as_str());
        }

        #[test]
        fn any_text_decodes_without_panicking(text in ".{0,60}") {
            let _ = decoded(&text);
        }
    }
}
