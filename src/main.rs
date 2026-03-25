use agentgrep::cli::{Cli, Command};
use agentgrep::find::run_find;
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
                            serde_json::to_string_pretty(&result).expect("serialize grep json")
                        );
                    } else {
                        println!("query: {}", result.query);
                        println!(
                            "matches: {} in {} files",
                            result.total_matches, result.total_files
                        );
                        for file in result.files {
                            println!();
                            println!("{}", file.path);
                            println!("  matches:");
                            for line_match in file.matches {
                                println!("    - @ {}", line_match.line_number);
                                println!("      {}", line_match.line_text);
                            }
                        }
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
                println!("query: {}", result.query);
                println!("top files: {}", result.files.len());
                for (idx, file) in result.files.iter().enumerate() {
                    println!();
                    println!("{}. {}", idx + 1, file.path);
                    println!("   role: {}", file.role);
                    println!("   why:");
                    for reason in &file.why {
                        println!("     - {reason}");
                    }
                    println!("   structure:");
                    for item in &file.structure.items {
                        println!(
                            "     - {} {} @ {}-{} ({} lines)",
                            item.kind, item.label, item.start_line, item.end_line, item.line_count
                        );
                    }
                    if file.structure.omitted_count > 0 {
                        println!("     ... {} more symbols", file.structure.omitted_count);
                    }
                }
            }
        }
        Command::Smart(args) => match parse_smart_query(&args.terms) {
            Ok(query) => {
                let root = resolve_root(&args.path);
                let result = run_smart(&root, &query, &args);
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result).expect("serialize smart json")
                    );
                } else {
                    println!("query parameters:");
                    println!("  subject: {}", result.query.subject);
                    println!("  relation: {}", result.query.relation.as_str());
                    if !result.query.support.is_empty() {
                        println!("  support: {}", result.query.support.join(", "));
                    }
                    if let Some(kind) = &result.query.kind {
                        println!("  kind: {kind}");
                    }
                    if let Some(path_hint) = &result.query.path_hint {
                        println!("  path_hint: {path_hint}");
                    }
                    println!();
                    println!(
                        "top results: {} files, {} regions",
                        result.summary.total_files, result.summary.total_regions
                    );
                    if let Some(best_file) = &result.summary.best_file {
                        println!("best answer likely in {best_file}");
                    }
                    for (idx, file) in result.files.iter().enumerate() {
                        println!();
                        println!("{}. {}", idx + 1, file.path);
                        println!("   role: {}", file.role);
                        println!("   why:");
                        for reason in &file.why {
                            println!("     - {reason}");
                        }
                        println!("   structure:");
                        for item in &file.structure.items {
                            println!(
                                "     - {} {} @ {}-{} ({} lines)",
                                item.kind,
                                item.label,
                                item.start_line,
                                item.end_line,
                                item.line_count
                            );
                        }
                        if file.structure.omitted_count > 0 {
                            println!("     ... {} more symbols", file.structure.omitted_count);
                        }
                        println!("   regions:");
                        for region in &file.regions {
                            println!(
                                "     - {} @ {}-{} ({} lines)",
                                region.label, region.start_line, region.end_line, region.line_count
                            );
                            println!("       kind: {}", region.kind);
                            if region.full_region {
                                println!("       full region:");
                            } else {
                                println!("       snippet:");
                            }
                            for line in region.body.lines() {
                                println!("         {line}");
                            }
                            println!("       why:");
                            for reason in &region.why {
                                println!("         - {reason}");
                            }
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                eprintln!();
                eprintln!("smart queries use a small DSL. Example:");
                eprintln!("  agentgrep smart subject:auth_status relation:rendered support:ui");
                std::process::exit(2);
            }
        },
    }
}
