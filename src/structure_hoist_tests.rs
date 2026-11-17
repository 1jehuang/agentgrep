use super::*;
use std::hint::black_box;
use std::time::Instant;

fn legacy_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let mut rest = strip_rust_visibility(line).trim_start();
    if keyword == "fn" {
        rest = strip_keyword_prefix(rest, "async")
            .unwrap_or(rest)
            .trim_start();
    }
    parse_keyword_identifier(rest, keyword)
}

fn legacy(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if crate::control::checkpoint().is_err() {
            break;
        }
        let trimmed = line.trim_start();
        if let Some(label) = legacy_keyword(trimmed, "fn") {
            items.push(structure_item("function", label, idx + 1));
            continue;
        }
        if let Some(label) = legacy_keyword(trimmed, "struct") {
            items.push(structure_item("struct", label, idx + 1));
            continue;
        }
        if let Some(label) = legacy_keyword(trimmed, "enum") {
            items.push(structure_item("enum", label, idx + 1));
            continue;
        }
        if let Some(label) = legacy_keyword(trimmed, "trait") {
            items.push(structure_item("trait", label, idx + 1));
            continue;
        }
        if let Some(label) = parse_rust_impl_item(trimmed) {
            items.push(structure_item("impl", label, idx + 1));
        }
    }
    items
}

#[test]
fn visibility_hoist_matches_old_parser() {
    let mut text = String::new();
    for indent in ["", " ", "\t", "\u{2003}"] {
        for visibility in [
            "",
            "pub ",
            "pub(crate) ",
            "pub(in super::x) ",
            "pub((x)) ",
            "pub( ",
            "public ",
            "pub\u{2003}",
        ] {
            for item in [
                "fn f() {}",
                "async fn f() {}",
                "struct S;",
                "enum E {}",
                "trait T {}",
                "impl<T> T {}",
                "pub fn nested() {}",
                "",
                "// fn comment() {}",
            ] {
                text.push_str(&format!("{indent}{visibility}{item}\n"));
            }
        }
    }
    for source in [
        text.as_str(),
        include_str!("search.rs"),
        include_str!("structure.rs"),
    ] {
        assert_eq!(legacy(source), extract_rust(source));
    }
}

#[test]
fn visibility_hoist_preserves_resource_errors() {
    use crate::control::{SearchControl, SearchLimits, with_search_control};
    use std::sync::{Arc, atomic::AtomicBool};
    let source = "pub struct A;\npub(crate) fn f() {}\nenum E {}\nimpl E {}\n";
    for limit in 0..6 {
        let run = |parse: fn(&str) -> Vec<StructureItem>| {
            let limits = SearchLimits {
                max_structure_items: limit,
                ..SearchLimits::default()
            };
            let control = SearchControl::new(Arc::new(AtomicBool::new(false)), limits);
            let mut observed = Vec::new();
            let result = with_search_control(control, || observed = parse(source));
            (observed, result)
        };
        assert_eq!(run(legacy), run(extract_rust));
    }
}

#[test]
#[ignore = "paired same-profile parser microbenchmark"]
fn visibility_hoist_benchmark() {
    let source = include_str!("search.rs");
    assert_eq!(legacy(source), extract_rust(source));
    let measure = |f: fn(&str) -> Vec<StructureItem>| {
        let start = Instant::now();
        for _ in 0..100 {
            black_box(f(black_box(source)));
        }
        start.elapsed()
    };
    for round in 0..8 {
        let (old, new) = if round % 2 == 0 {
            (measure(legacy), measure(extract_rust))
        } else {
            let new = measure(extract_rust);
            (measure(legacy), new)
        };
        eprintln!("visibility paired {round}: old={old:?} new={new:?}");
    }
}
