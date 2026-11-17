//! Representative exact structured-output parity between implicit and explicit scopes.
use agentgrep::cli::{Cli, Command};
use agentgrep::control::{SearchControl, SearchLimits, with_search_control};
use clap::Parser;
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool};

fn run(root: &Path, args: &[&str]) -> Value {
    let cli =
        Cli::try_parse_from(std::iter::once("agentgrep").chain(args.iter().copied())).unwrap();
    match cli.command {
        Command::Grep(args) => {
            serde_json::to_value(agentgrep::search::run_grep(root, &args).unwrap().to_json())
                .unwrap()
        }
        Command::Find(args) => {
            serde_json::to_value(agentgrep::find::run_find(root, &args).unwrap()).unwrap()
        }
        Command::Outline(args) => {
            serde_json::to_value(agentgrep::outline::run_outline(root, &args).unwrap()).unwrap()
        }
        Command::Trace(args) => {
            let query = agentgrep::smart_dsl::parse_smart_query(&args.terms).unwrap();
            serde_json::to_value(agentgrep::smart_engine::run_smart(root, &query, &args).unwrap())
                .unwrap()
        }
    }
}

#[test]
fn representative_search_outputs_match_across_scopes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("needle.rs"),
        "pub fn needle() {}\nfn caller() { needle(); }\n",
    )
    .unwrap();
    std::fs::write(root.path().join("other.rs"), "fn other() {}\n").unwrap();
    for args in [
        vec!["grep", "needle"],
        vec!["grep", "fn", "--regex"],
        vec!["find", "needle"],
        vec!["outline", "needle.rs"],
        vec!["trace", "subject:needle", "relation:defined"],
        vec!["grep", "no_such_symbol"],
    ] {
        let expected = run(root.path(), &args);
        let actual = with_search_control(
            SearchControl::new(Arc::new(AtomicBool::new(false)), SearchLimits::default()),
            || run(root.path(), &args),
        )
        .unwrap();
        assert_eq!(actual, expected, "{args:?}");
        assert_eq!(run(root.path(), &args), expected, "restored {args:?}");
    }
}
