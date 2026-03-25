use agentgrep::cli::{Cli, Command};
use agentgrep::search::run_grep;
use agentgrep::smart_dsl::parse_smart_query;
use clap::Parser;
use std::env;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Grep(args) => {
            let root = env::current_dir().expect("current directory");
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
            let query = args.query_parts.join(" ");
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "mode": "find",
                        "query": query,
                        "type": args.file_type,
                        "max_files": args.max_files,
                        "hidden": args.hidden,
                        "no_ignore": args.no_ignore,
                        "status": "not_implemented"
                    })
                );
            } else {
                println!("agentgrep find scaffold");
                println!("  query: {query}");
                println!("  max_files: {}", args.max_files);
                if let Some(file_type) = args.file_type {
                    println!("  type: {file_type}");
                }
                println!("  status: not implemented yet");
            }
        }
        Command::Smart(args) => match parse_smart_query(&args.terms) {
            Ok(query) => {
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "mode": "smart",
                            "query": query,
                            "max_files": args.max_files,
                            "max_regions": args.max_regions,
                            "full_region": format!("{:?}", args.full_region).to_lowercase(),
                            "debug_plan": args.debug_plan,
                            "status": "not_implemented"
                        }))
                        .expect("serialize smart json")
                    );
                } else {
                    println!("agentgrep smart scaffold");
                    println!("  subject: {}", query.subject);
                    println!("  relation: {}", query.relation.as_str());
                    if !query.support.is_empty() {
                        println!("  support: {}", query.support.join(", "));
                    }
                    if let Some(kind) = query.kind {
                        println!("  kind: {kind}");
                    }
                    if let Some(path_hint) = query.path_hint {
                        println!("  path_hint: {path_hint}");
                    }
                    println!("  max_files: {}", args.max_files);
                    println!("  max_regions: {}", args.max_regions);
                    println!("  full_region: {:?}", args.full_region);
                    println!("  status: not implemented yet");
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
