//! Checks the method table against the official parameter snapshot and keeps
//! `docs/endpoints.md` in step with the table.
//!
//! Run with `SCROBL_BLESS=1` to rewrite the generated block of the document.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;
use std::fmt::Write as _;

use scrobl::protocol::methods;
use scrobl::protocol::{Auth, MethodSpec, Paging, Requirement, Verb};
use serde_json::Value;

const SNAPSHOT: &str = include_str!("../fixtures/official/method-parameters.json");

/// Names the library adds itself, so no method lists them.
const IMPLIED: &[&str] = &["api_key", "api_sig", "sk", "method", "format", "callback"];

/// Methods whose typed model or verification level is above the default, as
/// `(method, typed model, status)`. Every other method is `—` and `inventoried`.
const LEVELS: &[(&str, &str, &str)] = &[];

const BEGIN: &str = "<!-- BEGIN GENERATED -->";
const END: &str = "<!-- END GENERATED -->";

struct SnapshotParam {
    documented: String,
    name: String,
    label: String,
}

impl SnapshotParam {
    fn indexed(&self) -> bool {
        self.documented.ends_with("[i]")
    }

    fn requirement(&self, method: &str) -> Requirement {
        match self.label.as_str() {
            "(Required)" => Requirement::Required,
            "(Optional)" => Requirement::Optional,
            "(Required (unless mbid)]" => Requirement::Conditional,
            other => panic!("{method}: {}: unknown label {other:?}", self.name),
        }
    }
}

struct SnapshotMethod {
    name: String,
    params: Vec<SnapshotParam>,
}

impl SnapshotMethod {
    fn has(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }

    fn own_params(&self) -> impl Iterator<Item = &SnapshotParam> {
        self.params
            .iter()
            .filter(|p| !IMPLIED.contains(&p.name.as_str()))
    }
}

fn snapshot() -> Vec<SnapshotMethod> {
    let root: Value = serde_json::from_str(SNAPSHOT).unwrap();
    root["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| SnapshotMethod {
            name: m["name"].as_str().unwrap().to_owned(),
            params: m["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let documented = p["name"].as_str().unwrap().to_owned();
                    SnapshotParam {
                        name: p["normalized"]
                            .as_str()
                            .map_or_else(|| documented.clone(), str::to_owned),
                        documented,
                        label: p["label"].as_str().unwrap().to_owned(),
                    }
                })
                .collect(),
        })
        .collect()
}

#[test]
fn table_names_match_snapshot() {
    let snapshot = snapshot();
    assert_eq!(snapshot.len(), 57, "snapshot method count");
    assert_eq!(methods::ALL.len(), 57, "table method count");
    for (index, (spec, page)) in methods::ALL.iter().zip(&snapshot).enumerate() {
        assert_eq!(spec.name, page.name, "method #{index}: name");
    }
    let unique: HashSet<_> = methods::ALL.iter().map(|m| m.name).collect();
    assert_eq!(
        unique.len(),
        methods::ALL.len(),
        "table has a duplicate name"
    );
}

#[test]
fn table_matches_snapshot() {
    for page in &snapshot() {
        let spec = methods::by_name(&page.name).unwrap();
        let method = &page.name;

        let documented: Vec<_> = page.own_params().collect();
        assert_eq!(
            spec.params.len(),
            documented.len(),
            "{method}: params: table has {:?}, snapshot has {:?}",
            spec.params.iter().map(|p| p.name).collect::<Vec<_>>(),
            documented.iter().map(|p| &p.name).collect::<Vec<_>>(),
        );
        for (index, (param, doc)) in spec.params.iter().zip(&documented).enumerate() {
            assert_eq!(param.name, doc.name, "{method}: params[{index}].name");
            assert_eq!(
                param.requirement,
                doc.requirement(method),
                "{method}: {}: requirement",
                doc.name
            );
            assert_eq!(
                param.indexed,
                doc.indexed(),
                "{method}: {}: indexed",
                doc.name
            );
        }

        let expected_auth = if page.has("sk") {
            Auth::Session
        } else if page.has("api_sig") {
            Auth::Signed
        } else {
            Auth::ApiKey
        };
        assert_eq!(spec.auth, expected_auth, "{method}: auth");

        let expected_verb = if spec.write || method == "auth.getMobileSession" {
            Verb::Post
        } else {
            Verb::Get
        };
        assert_eq!(spec.verb, expected_verb, "{method}: verb");
        assert!(
            !spec.write || spec.auth == Auth::Session,
            "{method}: write needs a session"
        );

        let expected_paging = match (page.has("page"), page.has("limit")) {
            (true, true) => Paging::PageAndLimit,
            (false, true) => Paging::LimitOnly,
            (false, false) => Paging::None,
            (true, false) => panic!("{method}: documents page without limit"),
        };
        assert_eq!(spec.paging, expected_paging, "{method}: paging");
    }
}

#[test]
fn class_counts() {
    let count = |keep: fn(&MethodSpec) -> bool| methods::ALL.iter().filter(|m| keep(m)).count();
    assert_eq!(
        count(|m| m.auth == Auth::ApiKey && !m.write),
        44,
        "key-only reads"
    );
    assert_eq!(
        count(|m| m.auth == Auth::Session && m.write),
        10,
        "session writes"
    );
    assert_eq!(count(|m| m.auth == Auth::Session), 10, "session methods");
    let signed: Vec<_> = methods::ALL
        .iter()
        .filter(|m| m.auth == Auth::Signed)
        .map(|m| m.name)
        .collect();
    assert_eq!(
        signed,
        ["auth.getMobileSession", "auth.getSession", "auth.getToken"],
        "signed methods without a session"
    );
}

#[test]
fn by_name_ignores_ascii_case() {
    for spec in methods::ALL {
        for name in [
            spec.name.to_owned(),
            spec.name.to_ascii_lowercase(),
            spec.name.to_ascii_uppercase(),
        ] {
            let found = methods::by_name(&name);
            assert_eq!(found.map(|m| m.name), Some(spec.name), "by_name({name:?})");
        }
    }
    assert!(methods::by_name("user.getNothing").is_none());
    assert!(methods::by_name("").is_none());
}

fn level(method: &str) -> (&'static str, &'static str) {
    LEVELS
        .iter()
        .find(|(name, ..)| *name == method)
        .map_or(("—", "inventoried"), |&(_, model, status)| {
            (model, status)
        })
}

fn credentials(auth: Auth) -> &'static str {
    match auth {
        Auth::ApiKey => "API key",
        Auth::Signed => "API key, signature",
        Auth::Session => "API key, signature, session",
    }
}

fn paging(paging: Paging) -> &'static str {
    match paging {
        Paging::None => "—",
        Paging::LimitOnly => "limit",
        Paging::PageAndLimit => "page, limit",
    }
}

fn parameters(spec: &MethodSpec) -> String {
    if spec.params.is_empty() {
        return "—".to_owned();
    }
    let cells: Vec<_> = spec
        .params
        .iter()
        .map(|p| {
            let name = if p.indexed {
                format!("{}[i]", p.name)
            } else {
                p.name.to_owned()
            };
            match p.requirement {
                Requirement::Required => name,
                Requirement::Conditional => format!("{name}\\*"),
                Requirement::Optional => format!("_{name}_"),
            }
        })
        .collect();
    cells.join(", ")
}

fn generated_table() -> String {
    let mut out = String::new();
    out.push_str(
        "| Method | Verb | Credentials | Write | Paging | Parameters | Typed model | Status |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    let mut package = "";
    for spec in methods::ALL {
        let (this_package, _) = spec.name.split_once('.').unwrap();
        if this_package != package {
            package = this_package;
            writeln!(out, "| **{package}** | | | | | | | |").unwrap();
        }
        let (model, status) = level(spec.name);
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |",
            spec.name,
            if spec.verb == Verb::Post {
                "POST"
            } else {
                "GET"
            },
            credentials(spec.auth),
            if spec.write { "yes" } else { "—" },
            paging(spec.paging),
            parameters(spec),
            model,
            status,
        )
        .unwrap();
    }
    out
}

#[test]
fn endpoints_doc_is_in_sync() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/endpoints.md");
    let doc = std::fs::read_to_string(path).unwrap();
    let start = doc.find(BEGIN).expect("missing BEGIN GENERATED marker") + BEGIN.len();
    let end = doc.find(END).expect("missing END GENERATED marker");
    assert!(start <= end, "generated markers are out of order");

    let block = format!("\n\n{}\n", generated_table());
    if std::env::var_os("SCROBL_BLESS").is_some_and(|v| v == "1") {
        let blessed = format!("{}{block}{}", &doc[..start], &doc[end..]);
        std::fs::write(path, blessed).unwrap();
        return;
    }
    let current = &doc[start..end];
    if current != block {
        let differs = current
            .lines()
            .zip(block.lines())
            .find(|(have, want)| have != want);
        panic!(
            "docs/endpoints.md is out of date; rerun with SCROBL_BLESS=1\n  file:  {:?}\n  table: {:?}",
            differs.map(|(have, _)| have),
            differs.map(|(_, want)| want),
        );
    }
}
