use agentgrep::cli::{Cli, Command};
use agentgrep::find::run_find;
use agentgrep::outline::run_outline;
use agentgrep::render::{
    render_find_output, render_grep_output, render_outline_output, render_smart_output,
};
use agentgrep::search::run_grep;
use agentgrep::smart_dsl::parse_smart_query;
use agentgrep::smart_engine::run_smart;
use clap::Parser;
use std::path::PathBuf;

fn resolve_root(path: &Option<String>) -> PathBuf {
    match path {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir().expect("current directory"),
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Grep(args) => {
            let root = resolve_root(&args.path);
            match run_grep(&root, &args) {
                Ok(result) => {
                    if args.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result.to_json())
                                .expect("serialize grep json")
                        );
                    } else {
                        println!("{}", render_grep_output(&result, &args, None));
                    }
                }
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(2);
                }
            }
        }
        Command::Find(args) => {
            let root = resolve_root(&args.path);
            let result = run_find(&root, &args);
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).expect("serialize find json")
                );
            } else {
                println!("{}", render_find_output(&result, &args));
            }
        }
        Command::Outline(args) => {
            let root = resolve_root(&args.path);
            match run_outline(&root, &args) {
                Ok(result) => {
                    if args.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result).expect("serialize outline json")
                        );
                    } else {
                        println!("{}", render_outline_output(&result));
                    }
                }
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(2);
                }
            }
        }
        Command::Trace(args) => match parse_smart_query(&args.terms) {
            Ok(query) => {
                let root = resolve_root(&args.path);
                match run_smart(&root, &query, &args) {
                    Ok(result) => {
                        if args.json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&result)
                                    .expect("serialize trace json")
                            );
                        } else {
                            println!("{}", render_smart_output(&result, &args));
                        }
                    }
                    Err(err) => {
                        eprintln!("error: {err}");
                        std::process::exit(2);
                    }
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                eprintln!();
                eprintln!("trace queries use a small DSL. Example:");
                eprintln!("  agentgrep trace subject:auth_status relation:rendered support:ui");
                std::process::exit(2);
            }
        },
    }
}
