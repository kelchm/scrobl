#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::{Value, json};

use super::*;

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
