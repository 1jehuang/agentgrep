#!/usr/bin/env python3
"""Benchmark agentgrep grep vs rg with hyperfine over the shared matrix.

Runs `hyperfine -N --warmup 3 --runs 15` per (corpus, case, mode) pair,
exports per-cell JSON into bench-results/hyperfine/, writes an aggregate
bench-results/bench_vs_rg.json, and prints a speedup table
(agentgrep_mean / rg_mean; >1.0 means agentgrep is slower).
"""

from __future__ import annotations

import datetime
import json
import os
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench_cases  # noqa: E402

WARMUP = int(os.environ.get("AGENTGREP_BENCH_WARMUP", "3"))
RUNS = int(os.environ.get("AGENTGREP_BENCH_RUNS", "15"))


def main() -> int:
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    bin_path = os.environ.get(
        "AGENTGREP_BIN", os.path.join(root_dir, "target", "release", "agentgrep")
    )
    if not shutil.which("hyperfine"):
        print("error: hyperfine is required", file=sys.stderr)
        return 1

    out_dir = os.path.join(root_dir, "bench-results")
    hf_dir = os.path.join(out_dir, "hyperfine")
    os.makedirs(hf_dir, exist_ok=True)

    rows = []
    for corpus in bench_cases.corpora():
        if not os.path.isdir(corpus.path):
            print(f"!! skipping corpus {corpus.id}: missing {corpus.path}")
            continue
        for case in bench_cases.cases():
            for mode in ("match", "paths_only"):
                paths_only = mode == "paths_only"
                ag_cmd = bench_cases.shell_join(
                    bench_cases.agentgrep_argv(
                        bin_path, case, corpus, paths_only=paths_only, json_out=True
                    )
                )
                rg_cmd = bench_cases.shell_join(
                    bench_cases.rg_argv(
                        case, corpus, paths_only=paths_only, json_out=not paths_only
                    )
                )
                cell = f"{corpus.id}__{case.id}__{mode}"
                export = os.path.join(hf_dir, f"{cell}.json")
                print(f"== {cell} ==")
                proc = subprocess.run(
                    [
                        "hyperfine",
                        "-N",
                        "--warmup",
                        str(WARMUP),
                        "--runs",
                        str(RUNS),
                        "--ignore-failure",  # rg exits 1 on zero matches
                        "--export-json",
                        export,
                        "--command-name",
                        f"agentgrep {cell}",
                        ag_cmd,
                        "--command-name",
                        f"rg {cell}",
                        rg_cmd,
                    ],
                    capture_output=True,
                    text=True,
                )
                if proc.returncode != 0:
                    print(f"hyperfine failed for {cell}: {proc.stderr.strip()[:300]}")
                    rows.append(
                        {"cell": cell, "error": proc.stderr.strip()[:300]}
                    )
                    continue
                with open(export) as fh:
                    data = json.load(fh)
                ag_res, rg_res = data["results"][0], data["results"][1]
                rows.append(
                    {
                        "cell": cell,
                        "corpus": corpus.id,
                        "case": case.id,
                        "mode": mode,
                        "agentgrep_mean_s": ag_res["mean"],
                        "agentgrep_stddev_s": ag_res.get("stddev"),
                        "rg_mean_s": rg_res["mean"],
                        "rg_stddev_s": rg_res.get("stddev"),
                        "ratio_ag_over_rg": ag_res["mean"] / rg_res["mean"],
                        "agentgrep_cmd": ag_cmd,
                        "rg_cmd": rg_cmd,
                    }
                )

    report = {
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "agentgrep_bin": bin_path,
        "warmup": WARMUP,
        "runs": RUNS,
        "rows": rows,
    }
    agg_path = os.path.join(out_dir, "bench_vs_rg.json")
    with open(agg_path, "w") as fh:
        json.dump(report, fh, indent=2)

    print()
    print("Speedup table (ratio = agentgrep_mean / rg_mean; >1.00 = agentgrep slower)")
    header = f"{'corpus':<10} {'case':<20} {'mode':<11} {'ag_ms':>9} {'rg_ms':>9} {'ratio':>7}"
    print(header)
    print("-" * len(header))
    ok_rows = [r for r in rows if "error" not in r]
    for r in ok_rows:
        print(
            f"{r['corpus']:<10} {r['case']:<20} {r['mode']:<11}"
            f" {r['agentgrep_mean_s'] * 1000:>9.2f} {r['rg_mean_s'] * 1000:>9.2f}"
            f" {r['ratio_ag_over_rg']:>7.2f}"
        )
    if ok_rows:
        import statistics

        geo = statistics.geometric_mean(
            [r["ratio_ag_over_rg"] for r in ok_rows]
        )
        print("-" * len(header))
        print(f"geometric mean ratio: {geo:.2f}")
        report["geometric_mean_ratio"] = geo
        with open(agg_path, "w") as fh:
            json.dump(report, fh, indent=2)
    print(f"\nresults: {agg_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
