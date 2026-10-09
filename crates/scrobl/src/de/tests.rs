#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;

const RECORDED: &[u8] =
    include_bytes!("../../fixtures/recorded/lastfm-0.10.0-recent-tracks-extended-trimmed.json");

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn uint_accepts_integers_and_digit_strings() {
    assert_eq!(uint(&json!(0)), Some(0));
    assert_eq!(uint(&json!(1783050940_u64)), Some(1_783_050_940));
    assert_eq!(uint(&json!(u64::MAX)), Some(u64::MAX));
    assert_eq!(uint(&json!("0")), Some(0));
    assert_eq!(uint(&json!("200")), Some(200));
    assert_eq!(uint(&json!("007")), Some(7));
    assert_eq!(uint(&json!("18446744073709551615")), Some(u64::MAX));
}

#[test]
fn uint_rejects_everything_else() {
    let rejected = [
        parse("1.0"),
        parse("1.5"),
        parse("1e3"),
        parse("-1"),
        parse("-0"),
        parse("18446744073709551616"),
        json!(""),
        json!(" 1"),
        json!("1 "),
        json!("+1"),
        json!("-1"),
        json!("1.0"),
        json!("1e3"),
        json!("0x10"),
        json!("1_000"),
        json!("١٢٣"),
        json!("18446744073709551616"),
        json!(true),
        json!(false),
        Value::Null,
        json!([]),
        json!(["1"]),
        json!({}),
        json!({"uts": "1"}),
    ];
    for value in rejected {
        assert_eq!(uint(&value), None, "uint({value})");
    }
}

#[test]
fn flag_accepts_zero_and_one_only() {
    assert_eq!(flag(&json!("0")), Some(false));
    assert_eq!(flag(&json!("1")), Some(true));
    assert_eq!(flag(&json!(0)), Some(false));
    assert_eq!(flag(&json!(1)), Some(true));
}

#[test]
fn flag_rejects_everything_else() {
    let rejected = [
        json!("2"),
        json!("01"),
        json!(""),
        json!(" 1"),
        json!("true"),
        json!("false"),
        json!("TRUE"),
        json!(2),
        json!(-1),
        parse("1.0"),
        json!(true),
        json!(false),
        Value::Null,
        json!([]),
        json!({}),
    ];
    for value in rejected {
        assert_eq!(flag(&value), None, "flag({value})");
    }
}

#[test]
fn flag_or_word_adds_exactly_true_and_false() {
    assert_eq!(flag_or_word(&json!("true")), Some(true));
    assert_eq!(flag_or_word(&json!("false")), Some(false));
    assert_eq!(flag_or_word(&json!("1")), Some(true));
    assert_eq!(flag_or_word(&json!(0)), Some(false));
    for value in [
        json!("True"),
        json!("TRUE"),
        json!(" true"),
        json!("yes"),
        json!(""),
        json!("2"),
        json!(true),
        json!(false),
        Value::Null,
        json!({}),
    ] {
        assert_eq!(flag_or_word(&value), None, "flag_or_word({value})");
    }
}

#[test]
fn one_or_many_wraps_an_object_and_keeps_an_array() {
    let object = json!({"name": "x"});
    assert_eq!(one_or_many(object.clone()), Some(vec![object]));

    let array = json!([{"name": "a"}, {"name": "b"}]);
    assert_eq!(
        one_or_many(array),
        Some(vec![json!({"name": "a"}), json!({"name": "b"})])
    );
    assert_eq!(one_or_many(json!([])), Some(vec![]));
}

#[test]
fn one_or_many_rejects_scalars() {
    for value in [json!(""), json!("x"), json!(1), json!(true), Value::Null] {
        assert_eq!(one_or_many(value.clone()), None, "one_or_many({value})");
    }
}

// The strict parser: serde_json's own Value, but a repeated member name is an
// error instead of the last one winning.

fn strict(text: &str) -> Result<Value, StrictError> {
    strict_value(text.as_bytes())
}

#[track_caller]
fn assert_duplicate(text: &str) {
    match strict(text) {
        Err(StrictError::Duplicate { line, column }) => {
            assert!(line >= 1 && column >= 1, "{text}");
        }
        other => panic!("`{text}` was not rejected as a duplicate: {other:?}"),
    }
}

#[test]
fn the_strict_parser_agrees_with_serde_json_on_the_recorded_fixture() {
    let expected: Value = serde_json::from_slice(RECORDED).unwrap();
    assert_eq!(strict_value(RECORDED).unwrap(), expected);
}

#[test]
fn the_strict_parser_agrees_with_serde_json_on_scalars_and_edge_cases() {
    for text in [
        "null",
        "true",
        " false ",
        "0",
        "-0",
        "1.0",
        "1e3",
        "-1.5E-2",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        r#""""#,
        r#""plain""#,
        r#""esc \u00e9 \ud83c\udfb5 \n \" \\""#,
        "\"\u{e9}\u{1f3b5}\"",
        "[]",
        "{}",
        "[[],{},[{}]]",
        r#"{"a":{"a":{"a":[{"a":1},{"a":2}]}},"b":{"a":3}}"#,
        "\n\t{ \"k\" : [ 1 , 2 ] }\r\n",
    ] {
        let expected: Value = serde_json::from_str(text).unwrap();
        assert_eq!(strict(text).unwrap(), expected, "{text}");
    }
}

#[test]
fn a_repeated_member_is_rejected_at_every_depth() {
    for text in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":1,"b":2,"a":1}"#,
        r#"{"a":{"b":1},"a":{"b":1}}"#,
        r#"[{"a":1,"a":1}]"#,
        r#"{"x":[1,{"y":[{"z":{"k":1,"k":2}}]}]}"#,
        r#"{"x":{"k":1},"y":{"k":1,"k":1}}"#,
        r#"{"":1,"":2}"#,
    ] {
        assert_duplicate(text);
    }
}

#[test]
fn a_member_is_repeated_when_the_escaped_spelling_is_the_same_name() {
    assert_duplicate(r#"{"a":1,"\u0061":2}"#);
    assert_duplicate(r#"{"caf\u00e9":1,"café":2}"#);
}

#[test]
fn names_differing_in_case_or_whitespace_are_not_repeats() {
    let value = strict(r#"{"a":1,"A":2,"a ":3," a":4}"#).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
}

#[test]
fn a_duplicate_reports_where_without_quoting_the_document() {
    let Err(StrictError::Duplicate { line, column }) =
        strict("{\n  \"SYNTHETIC-RESPONSE-TEXT\": 1,\n  \"SYNTHETIC-RESPONSE-TEXT\": 2\n}")
    else {
        panic!("not a duplicate");
    };
    assert_eq!(line, 3);
    assert!(column > 1);
}

#[test]
fn malformed_json_is_an_error_but_not_a_duplicate() {
    for text in [
        "",
        " ",
        "{",
        "[1,",
        r#"{"a":1,}"#,
        r#"{"a" 1}"#,
        "nul",
        "1 2",
        r#"{"a":1} x"#,
        "{}{}",
        r#""unterminated"#,
    ] {
        assert!(
            matches!(strict(text), Err(StrictError::Json(_))),
            "`{text}` should be malformed JSON"
        );
        assert!(serde_json::from_str::<Value>(text).is_err(), "{text}");
    }
    assert!(matches!(
        strict_value(b"{\"a\": \"\xff\"}"),
        Err(StrictError::Json(_))
    ));
}

#[test]
fn nesting_is_limited_like_serde_json_and_never_overflows_the_stack() {
    for depth in [1, 100, 127, 128, 129, 200, 100_000] {
        let arrays = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let objects = format!("{}1{}", r#"{"k":"#.repeat(depth), "}".repeat(depth));
        for text in [arrays, objects] {
            let expected = serde_json::from_str::<Value>(&text);
            let got = strict(&text);
            assert_eq!(got.is_ok(), expected.is_ok(), "depth {depth}");
            if let (Ok(got), Ok(expected)) = (got, expected) {
                assert_eq!(got, expected);
            }
        }
    }
}

#[test]
fn a_duplicate_below_the_depth_limit_is_still_found() {
    let text = format!(
        "{}{}{}",
        r#"{"k":"#.repeat(100),
        r#"{"d":1,"d":2}"#,
        "}".repeat(100)
    );
    assert_duplicate(&text);
}

#[test]
fn a_large_object_without_repeats_parses() {
    let members: Vec<String> = (0..20_000).map(|i| format!(r#""key{i}": {i}"#)).collect();
    let text = format!("{{{}}}", members.join(","));
    let value = strict(&text).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 20_000);
    assert_eq!(value, serde_json::from_str::<Value>(&text).unwrap());
}

fn any_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        (-1.0e12_f64..1.0e12).prop_map(Value::from),
        ".{0,12}".prop_map(Value::from),
    ];
    leaf.prop_recursive(5, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::vec((".{0,6}", inner), 0..6)
                .prop_map(|members| Value::Object(members.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_strict_parser_yields_the_same_value_as_serde_json_without_duplicates(
        value in any_value()
    ) {
        for text in [
            serde_json::to_string(&value).unwrap(),
            serde_json::to_string_pretty(&value).unwrap(),
        ] {
            let expected: Value = serde_json::from_str(&text).unwrap();
            prop_assert_eq!(strict_value(text.as_bytes()).unwrap(), expected);
        }
    }

    #[test]
    fn a_damaged_document_never_succeeds_where_serde_json_fails(
        value in any_value(),
        cut in any::<prop::sample::Index>(),
        delete in any::<bool>(),
    ) {
        let text = serde_json::to_string(&value).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let at = cut.index(chars.len() + 1);
        let damaged: String = if delete {
            chars.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, c)| c).collect()
        } else {
            chars.iter().take(at).collect()
        };
        match (strict_value(damaged.as_bytes()), serde_json::from_str::<Value>(&damaged)) {
            (Ok(strict), Ok(std)) => prop_assert_eq!(strict, std),
            (Ok(_), Err(e)) => prop_assert!(false, "accepted what serde_json rejects: {e}"),
            // Deleting a character from a name can make two names equal.
            (Err(StrictError::Duplicate { .. }), Ok(_)) => {}
            (Err(StrictError::Duplicate { .. }), Err(_)) => {}
            (Err(StrictError::Json(_)), Ok(_)) => prop_assert!(false, "rejected valid JSON"),
            (Err(StrictError::Json(_)), Err(_)) => {}
        }
    }

    #[test]
    fn a_member_added_twice_is_always_rejected(
        value in any_value(),
        name in "[a-z]{1,6}",
    ) {
        // Splice two members of one name into an object at a random depth.
        let spliced = match value {
            Value::Object(mut object) => {
                object.remove(&name);
                let mut text = serde_json::to_string(&Value::Object(object)).unwrap();
                text.pop();
                let sep = if text.len() > 1 { "," } else { "" };
                format!(r#"{text}{sep}"{name}":1,"{name}":2}}"#)
            }
            other => format!(r#"[{},{{"{name}":1,"{name}":2}}]"#, serde_json::to_string(&other).unwrap()),
        };
        let rejected = matches!(
            strict_value(spliced.as_bytes()),
            Err(StrictError::Duplicate { .. })
        );
        prop_assert!(rejected, "{}", spliced.len());
    }
}
