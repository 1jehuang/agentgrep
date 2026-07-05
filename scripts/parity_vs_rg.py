#!/usr/bin/env python3
"""Parity checker: agentgrep grep vs ripgrep ground truth.

For every (corpus, case, mode) cell in the shared matrix this runs agentgrep
and rg, normalizes both outputs to relative paths, and compares:
  * match mode: set of matched files AND set of (file, line_number) pairs
  * paths-only mode: set of matched files

Writes bench-results/parity.json and prints a human summary. Exits nonzero on
any mismatch or execution error.
"""

from __future__ import annotations

import datetime
import json
import os
import subprocess
import sys
from typing import Dict, List, Set, Tuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench_cases  # noqa: E402

TIMEOUT_SECONDS = 300
SAMPLE_LIMIT = 10


def run(argv: List[str]) -> Tuple[int, str, str]:
    proc = subprocess.run(
        argv, capture_output=True, text=True, timeout=TIMEOUT_SECONDS
    )
    return proc.returncode, proc.stdout, proc.stderr


def relpath(path: str, root: str) -> str:
    if os.path.isabs(path):
        return os.path.relpath(path, root)
    # agentgrep emits paths relative to --path root; rg may emit
    # "<root>/rel" when root is a relative path.
    root_prefix = root.rstrip("/") + "/"
    if path.startswith(root_prefix):
        return path[len(root_prefix):]
    return path


def agentgrep_matches(
    bin_path: str, case, corpus
) -> Tuple[Set[str], Set[Tuple[str, int]], str]:
    argv = bench_cases.agentgrep_argv(
        bin_path, case, corpus, paths_only=False, json_out=True
    )
    code, out, err = run(argv)
    if code != 0:
        return set(), set(), f"agentgrep exit {code}: {err.strip()[:300]}"
    try:
        doc = json.loads(out)
    except json.JSONDecodeError as exc:
        return set(), set(), f"agentgrep bad json: {exc}"
    files: Set[str] = set()
    pairs: Set[Tuple[str, int]] = set()
    for f in doc.get("files", []):
        rel = relpath(f["path"], corpus.path)
        files.add(rel)
        for m in f.get("matches", []):
            pairs.add((rel, m["line_number"]))
    return files, pairs, ""


def agentgrep_paths_only(bin_path: str, case, corpus) -> Tuple[Set[str], str]:
    argv = bench_cases.agentgrep_argv(
        bin_path, case, corpus, paths_only=True, json_out=False
    )
    code, out, err = run(argv)
    if code != 0:
        return set(), f"agentgrep exit {code}: {err.strip()[:300]}"
    files = {
        relpath(line.strip(), corpus.path)
        for line in out.splitlines()
        if line.strip()
    }
    return files, ""


def rg_matches(case, corpus) -> Tuple[Set[str], Set[Tuple[str, int]], str]:
    argv = bench_cases.rg_argv(case, corpus, paths_only=False, json_out=True)
    code, out, err = run(argv)
    # Mirror agentgrep's rg exit handling (src/search.rs run_rg_output):
    # exit 2 with partial output (e.g. broken symlinks under --follow) is
    # still usable ground truth.
    if code not in (0, 1) and not (code == 2 and out.strip()):
        return set(), set(), f"rg exit {code}: {err.strip()[:300]}"
    files: Set[str] = set()
    pairs: Set[Tuple[str, int]] = set()
    for line in out.splitlines():
        try:
            evt = json.loads(line)
        except json.JSONDecodeError:
            continue
        if evt.get("type") != "match":
            continue
        data = evt["data"]
        path = data["path"].get("text")
        if path is None:
            continue  # non-utf8 path; agentgrep can't emit it in JSON either
        rel = relpath(path, corpus.path)
        files.add(rel)
        pairs.add((rel, data["line_number"]))
    return files, pairs, ""


def rg_paths_only(case, corpus) -> Tuple[Set[str], str]:
    argv = bench_cases.rg_argv(case, corpus, paths_only=True, json_out=False)
    code, out, err = run(argv)
    # Mirror agentgrep's rg exit handling (src/search.rs run_rg_paths_only).
    if code not in (0, 1) and not (code == 2 and out.strip()):
        return set(), f"rg exit {code}: {err.strip()[:300]}"
    files = {
        relpath(line.strip(), corpus.path)
        for line in out.splitlines()
        if line.strip()
    }
    return files, ""


def diff_summary(ag: Set, rg: Set) -> Dict:
    missing = sorted(rg - ag)
    extra = sorted(ag - rg)
    return {
        "agentgrep_count": len(ag),
        "rg_count": len(rg),
        "missing_in_agentgrep": len(missing),
        "extra_in_agentgrep": len(extra),
        "missing_sample": [str(x) for x in missing[:SAMPLE_LIMIT]],
        "extra_sample": [str(x) for x in extra[:SAMPLE_LIMIT]],
    }


def main() -> int:
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    bin_path = os.environ.get(
        "AGENTGREP_BIN", os.path.join(root_dir, "target", "release", "agentgrep")
    )
    results = []
    total = 0
    failed = 0
    for corpus in bench_cases.corpora():
        if not os.path.isdir(corpus.path):
            print(f"!! skipping corpus {corpus.id}: missing {corpus.path}")
            results.append(
                {"corpus": corpus.id, "skipped": True, "reason": "missing path"}
            )
            continue
        for case in bench_cases.cases():
            for mode in ("match", "paths_only"):
                total += 1
                label = f"{corpus.id:9s} {case.id:20s} {mode}"
                entry: Dict = {
                    "corpus": corpus.id,
                    "case": case.id,
                    "mode": mode,
                    "query": bench_cases.resolve_query(case, corpus),
                }
                errors = []
                if mode == "match":
                    ag_files, ag_pairs, ag_err = agentgrep_matches(
                        bin_path, case, corpus
                    )
                    rg_files, rg_pairs, rg_err = rg_matches(case, corpus)
                    if ag_err:
                        errors.append(ag_err)
                    if rg_err:
                        errors.append(rg_err)
                    entry["files"] = diff_summary(ag_files, rg_files)
                    entry["lines"] = diff_summary(ag_pairs, rg_pairs)
                    ok = (
                        not errors
                        and ag_files == rg_files
                        and ag_pairs == rg_pairs
                    )
                else:
                    ag_files, ag_err = agentgrep_paths_only(
                        bin_path, case, corpus
                    )
                    rg_files, rg_err = rg_paths_only(case, corpus)
                    if ag_err:
                        errors.append(ag_err)
                    if rg_err:
                        errors.append(rg_err)
                    entry["files"] = diff_summary(ag_files, rg_files)
                    ok = not errors and ag_files == rg_files
                entry["errors"] = errors
                entry["pass"] = ok
                results.append(entry)
                if ok:
                    print(f"PASS {label}")
                else:
                    failed += 1
                    print(f"FAIL {label}")
                    for e in errors:
                        print(f"     error: {e}")
                    fd = entry["files"]
                    print(
                        f"     files ag={fd['agentgrep_count']} rg={fd['rg_count']}"
                        f" missing={fd['missing_in_agentgrep']} extra={fd['extra_in_agentgrep']}"
                    )
                    if fd["missing_sample"]:
                        print(f"     missing sample: {fd['missing_sample'][:3]}")
                    if fd["extra_sample"]:
                        print(f"     extra sample: {fd['extra_sample'][:3]}")
                    if mode == "match":
                        ld = entry["lines"]
                        print(
                            f"     lines ag={ld['agentgrep_count']} rg={ld['rg_count']}"
                            f" missing={ld['missing_in_agentgrep']} extra={ld['extra_in_agentgrep']}"
                        )

    out_dir = os.path.join(root_dir, "bench-results")
    os.makedirs(out_dir, exist_ok=True)
    report = {
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "agentgrep_bin": bin_path,
        "total_checks": total,
        "failed_checks": failed,
        "pass": failed == 0,
        "results": results,
    }
    out_path = os.path.join(out_dir, "parity.json")
    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2)
    print()
    print(f"parity: {total - failed}/{total} checks passed -> {out_path}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
