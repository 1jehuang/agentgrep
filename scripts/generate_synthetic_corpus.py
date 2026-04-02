#!/usr/bin/env python3
import argparse
import os
import shutil
from pathlib import Path

RUST_TEMPLATE = """pub struct NoiseType{n};

pub fn noise_fn_{n}() -> &'static str {{
    \"noise-{n}\"
}}

pub fn helper_{n}() {{
    let _value = noise_fn_{n}();
}}
"""

TS_TEMPLATE = """export function noiseFn{n}() {{
  return 'noise-{n}';
}}

export const helper{n} = () => noiseFn{n}();
"""

MD_TEMPLATE = """# Noise Document {n}

## Overview

This document contains benchmark filler content {n}.

## Details

Additional filler lines for document {n}.
"""

TRANSCRIPT_RS = """pub enum TranscriptMode {
    Disabled,
    Live,
}

pub struct TranscriptController;

impl TranscriptController {
    pub fn start_live_transcription(&self) {
        println!(\"Live transcription started\");
    }

    pub fn stop_live_transcription(&self) {
        println!(\"Live transcription stopped\");
    }
}

pub fn render_transcript(mode: TranscriptMode) {
    match mode {
        TranscriptMode::Disabled => println!(\"transcript disabled\"),
        TranscriptMode::Live => println!(\"voice dictation speech transcript active\"),
    }
}
"""

TRANSCRIPT_TS = """export interface TranscriptState {
  transcript: string;
  voiceEnabled: boolean;
}

export function renderTranscript(state: TranscriptState) {
  return `speech ${state.transcript}`;
}

export const dictationHint = () => 'voice dictation transcript';
"""

TRACE_RS = """pub struct TranscriptMode;

pub fn build_transcript_mode() -> TranscriptMode {
    TranscriptMode
}

pub fn render_transcript_mode(mode: TranscriptMode) {
    let _ = mode;
}
"""

README = """# Synthetic agentgrep benchmark corpus

This corpus intentionally mixes many small files with a handful of relevant files.

## Voice transcription benchmark target

Search targets include transcript, voice, dictation, and speech.
"""


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def build_corpus(root: Path, small_files: int, doc_files: int) -> None:
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)

    write(root / "README.md", README)
    write(root / "src" / "ui" / "transcript_mode.rs", TRACE_RS)
    write(root / "src" / "app" / "dictation.rs", TRANSCRIPT_RS)
    write(root / "web" / "transcript.ts", TRANSCRIPT_TS)
    write(
        root / "docs" / "voice.md",
        "# Voice Input\n\n## Dictation\n\nSynthetic speech and transcript benchmark content.\n",
    )

    for idx in range(small_files):
        bucket = f"pkg_{idx % 40:02d}"
        write(root / "src" / bucket / f"noise_{idx:04d}.rs", RUST_TEMPLATE.format(n=idx))
        write(root / "web" / bucket / f"noise_{idx:04d}.ts", TS_TEMPLATE.format(n=idx))

    for idx in range(doc_files):
        bucket = f"section_{idx % 20:02d}"
        write(root / "docs" / bucket / f"doc_{idx:04d}.md", MD_TEMPLATE.format(n=idx))

    # Add some ignored bulk to exercise traversal behavior.
    write(root / ".gitignore", "build/\ncache/\n")
    for idx in range(200):
        write(root / "build" / f"ignored_{idx:04d}.txt", f"ignored transcript filler {idx}\n")
        write(root / "cache" / f"cache_{idx:04d}.txt", f"cached voice filler {idx}\n")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate synthetic agentgrep benchmark corpus")
    parser.add_argument("output", help="output directory")
    parser.add_argument("--small-files", type=int, default=1500)
    parser.add_argument("--doc-files", type=int, default=400)
    args = parser.parse_args()

    build_corpus(Path(args.output), args.small_files, args.doc_files)
    print(Path(args.output))


if __name__ == "__main__":
    main()
