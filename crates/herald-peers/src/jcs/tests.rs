use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;

fn written(text: &str) -> String {
    let value: Value = serde_json::from_str(text).unwrap_or_default();
    canonical(&value).unwrap_or_default()
}

fn double(value: f64) -> String {
    canonical(&json!(value)).unwrap_or_default()
}

#[test]
fn the_rfc_s_sample_is_written_as_the_rfc_writes_it() {
    let sample = r#"{
        "numbers": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
        "string": "\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/",
        "literals": [null, true, false]
    }"#;
    assert_eq!(
        written(sample),
        r#"{"literals":[null,true,false],"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],"string":"€$\u000f\nA'B\"\\\\\"/"}"#
    );
}

#[test]
fn members_are_sorted_by_their_names_utf16_code_units() {
    let sample = r#"{
        "\u20ac": "Euro Sign",
        "\r": "Carriage Return",
        "\ufb33": "Hebrew Letter Dalet With Dagesh",
        "1": "One",
        "\ud83d\ude00": "Emoji: Grinning Face",
        "\u0080": "Control",
        "\u00f6": "Latin Small Letter O With Diaeresis"
    }"#;
    assert_eq!(
        written(sample),
        "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",\
         \"\u{f6}\":\"Latin Small Letter O With Diaeresis\",\"\u{20ac}\":\"Euro Sign\",\
         \"\u{1f600}\":\"Emoji: Grinning Face\",\"\u{fb33}\":\"Hebrew Letter Dalet With Dagesh\"}"
    );
}

#[test]
fn every_short_escape_and_control_character_is_written_as_json_stringify_writes_it() {
    assert_eq!(
        canonical(&json!("\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}\u{2028}")).unwrap_or_default(),
        "\"\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}\u{2028}\""
    );
}

#[test]
fn arrays_and_objects_are_written_without_whitespace() {
    assert_eq!(
        written(r#" { "b" : [ 1 , { "d" : 2 , "c" : [ ] } ] , "a" : { } } "#),
        r#"{"a":{},"b":[1,{"c":[],"d":2}]}"#
    );
}

#[test]
fn doubles_are_written_as_ecmascript_writes_them() {
    for (value, text) in [
        (0.0, "0"),
        (-0.0, "0"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (-f64::MAX, "-1.7976931348623157e+308"),
        (9_007_199_254_740_994.0, "9007199254740994"),
        (999_999_999_999_999_700_000.0, "999999999999999700000"),
        (
            f64::from_bits(0x444b_1ae4_d6e2_ef4f),
            "999999999999999900000",
        ),
        (f64::from_bits(0x444b_1ae4_d6e2_ef50), "1e+21"),
        (1e20, "100000000000000000000"),
        (f64::from_bits(0x3eb0_c6f7_a0b5_ed8d), "0.000001"),
        (
            f64::from_bits(0x3eb0_c6f7_a0b5_ed8c),
            "9.999999999999997e-7",
        ),
        (1e-7, "1e-7"),
        (1.5e-7, "1.5e-7"),
        (0.1 + 0.2, "0.30000000000000004"),
        (-4.5, "-4.5"),
        (123.0, "123"),
        (1e30, "1e+30"),
        (1.25e30, "1.25e+30"),
    ] {
        assert_eq!(double(value), text, "{value:e}");
    }
}

#[test]
fn integers_are_written_as_ecmascript_writes_their_double() {
    for (value, text) in [
        (json!(0), "0"),
        (json!(-7), "-7"),
        (json!(9_007_199_254_740_992_u64), "9007199254740992"),
        (json!(-9_007_199_254_740_992_i64), "-9007199254740992"),
        (json!(9_007_199_254_740_994_u64), "9007199254740994"),
        (json!(36_028_797_018_963_964_u64), "36028797018963964"),
        (json!(1_152_921_504_606_847_000_u64), "1152921504606847000"),
        (json!(48_857_828_921_783_340_u64), "48857828921783340"),
        (json!(-(1_i64 << 54)), "-18014398509481984"),
    ] {
        assert_eq!(canonical(&value).unwrap_or_default(), text);
    }
}

#[test]
fn an_integer_read_as_a_double_written_otherwise_has_no_canonical_form() {
    for (value, number) in [
        (json!(9_007_199_254_740_993_u64), "9007199254740993"),
        (json!(-9_007_199_254_740_993_i64), "-9007199254740993"),
        (json!([{ "a": u64::MAX }]), "18446744073709551615"),
        (json!(36_028_797_018_963_966_u64), "36028797018963966"),
        (json!(1_152_921_504_606_846_976_u64), "1152921504606846976"),
    ] {
        let refused = canonical(&value);
        assert_eq!(
            refused,
            Err(Uncanonical::Inexact {
                number: number.to_owned()
            })
        );
        assert_eq!(
            refused.err().map(|why| why.to_string()).unwrap_or_default(),
            format!(
                "the integer {number} reads as a double written otherwise, so it has no canonical JSON form; write it as a string"
            )
        );
    }
}

fn values() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (-(1_i64 << 53)..=(1_i64 << 53)).prop_map(|number| json!(number)),
        any::<f64>()
            .prop_filter("finite", |number| number.is_finite())
            .prop_map(|number| json!(number)),
        any::<String>().prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(any::<String>(), inner, 0..4)
                .prop_map(|members| Value::Object(members.into_iter().collect())),
        ]
    })
}

proptest! {
    #[test]
    fn a_canonical_text_is_its_own_canonical_form(value in values()) {
        let text = canonical(&value).unwrap_or_default();
        let read: Value = serde_json::from_str(&text).unwrap_or_default();
        prop_assert_eq!(canonical(&read).unwrap_or_default(), text);
    }
}
