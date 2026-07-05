"""Shared corpus and query-case matrix for parity_vs_rg and bench_vs_rg.

Each case describes one agentgrep-grep invocation and its ripgrep ground-truth
equivalent. Both harness scripts import this module so the matrices never
drift apart.
"""

from __future__ import annotations

import os
import shlex
from dataclasses import dataclass, field
from typing import List, Optional


@dataclass(frozen=True)
class Corpus:
    id: str
    path: str
    rare_literal: str


@dataclass(frozen=True)
class Case:
    id: str
    query: str  # may be "@RARE" placeholder, replaced per corpus
    regex: bool = False
    file_type_ag: Optional[str] = None  # agentgrep --type value
    file_type_rg: Optional[str] = None  # rg --type value
    glob: Optional[str] = None
    hidden: bool = False


def corpora() -> List[Corpus]:
    return [
        Corpus(
            id="jcode",
            path=os.environ.get("AGENTGREP_JCODE_PATH", "/home/jeremy/jcode"),
            rare_literal="transcription",
        ),
        Corpus(
            id="linux",
            path=os.environ.get("AGENTGREP_LINUX_PATH", "/usr/src/linux"),
            rare_literal="kmalloc_array_node",
        ),
        Corpus(
            id="synthetic",
            path=os.environ.get(
                "AGENTGREP_SYNTH_PATH", "/tmp/agentgrep-synthetic-bench"
            ),
            rare_literal="start_live_transcription",
        ),
    ]


def cases() -> List[Case]:
    return [
        Case(id="common_literal", query="static"),
        Case(id="rare_literal", query="@RARE"),
        Case(id="zero_match_literal", query="zqxjv_no_match_sentinel_9317"),
        Case(id="dash_literal", query="->"),
        Case(
            id="regex_alternation",
            query="transcript|voice|dictation|speech",
            regex=True,
        ),
        Case(id="regex_class", query="[Tt]ranscript[a-z]*", regex=True),
        Case(
            id="type_rs",
            query="fn ",
            file_type_ag="rs",
            file_type_rg="rust",
        ),
        Case(id="glob_md", query="the", glob="*.md"),
        Case(id="hidden", query="static", hidden=True),
    ]


def resolve_query(case: Case, corpus: Corpus) -> str:
    return corpus.rare_literal if case.query == "@RARE" else case.query


def agentgrep_argv(
    bin_path: str, case: Case, corpus: Corpus, paths_only: bool, json_out: bool
) -> List[str]:
    argv = [bin_path, "grep"]
    if json_out:
        argv.append("--json")
    if paths_only:
        argv.append("--paths-only")
    if case.regex:
        argv.append("--regex")
    if case.file_type_ag:
        argv += ["--type", case.file_type_ag]
    if case.glob:
        argv += ["--glob", case.glob]
    if case.hidden:
        argv.append("--hidden")
    argv += ["--path", corpus.path, "--", resolve_query(case, corpus)]
    return argv


def rg_argv(
    case: Case, corpus: Corpus, paths_only: bool, json_out: bool
) -> List[str]:
    # Mirror the flags build_rg_command (src/search.rs) always passes so the
    # rg ground truth matches agentgrep's semantics: ignore user ripgrep
    # config, follow symlinks, and suppress per-file error noise.
    argv = ["rg", "--no-config", "--follow", "--no-messages"]
    if json_out:
        argv.append("--json")
    elif paths_only:
        argv.append("-l")
    else:
        argv.append("-n")
    if not case.regex:
        argv.append("-F")
    if case.file_type_rg:
        argv += ["--type", case.file_type_rg]
    if case.glob:
        argv += ["--glob", case.glob]
    if case.hidden:
        argv.append("--hidden")
    argv += ["--", resolve_query(case, corpus), corpus.path]
    return argv


def shell_join(argv: List[str]) -> str:
    return " ".join(shlex.quote(a) for a in argv)
