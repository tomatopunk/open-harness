#!/usr/bin/env python3
"""Validate inner-engine roadmap structure (R0 doc consistency gate).

Checks:
  - docs/inner-engine-roadmap.md has sections 1.1 / 1.2 / 1.3 and Plan R0–R4.
  - Completed subsection (1.1) table body must not contain gap/TODO markers.
  - Baseline docs referenced by the matrix exist.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ROADMAP = ROOT / "docs" / "inner-engine-roadmap.md"

FORBIDDEN_IN_COMPLETED = ("差距", "待做", "TODO", "FIXME", "WIP")


def main() -> int:
    if not ROADMAP.is_file():
        print(f"error: missing {ROADMAP}", file=sys.stderr)
        return 1

    text = ROADMAP.read_text(encoding="utf-8")

    for heading, label in (
        ("### 1.1", "已完成"),
        ("### 1.2", "刻意不做"),
        ("### 1.3", "迁宿主"),
    ):
        if heading not in text:
            print(f"error: roadmap missing {heading}", file=sys.stderr)
            return 1
        if label not in text:
            print(f"error: roadmap missing label {label!r}", file=sys.stderr)
            return 1

    for plan in ("Plan R0", "Plan R1", "Plan R2", "Plan R3", "Plan R4"):
        if plan not in text:
            print(f"error: roadmap missing {plan}", file=sys.stderr)
            return 1

    if "## 4." not in text and "Release Gate" not in text:
        print("error: roadmap missing Release Gate section", file=sys.stderr)
        return 1

    # Section 1.1 body: from ### 1.1 until ### 1.2
    m = re.search(r"### 1\.1[^\n]*\n(.*?)### 1\.2", text, re.DOTALL)
    if not m:
        print("error: could not parse §1.1 block", file=sys.stderr)
        return 1
    section_11 = m.group(1)
    for bad in FORBIDDEN_IN_COMPLETED:
        if bad in section_11:
            print(
                f"error: forbidden {bad!r} in roadmap §1.1 (已完成) body",
                file=sys.stderr,
            )
            return 1

    for rel in (
        "crates/agent-loop-runtime/ENGINE_BASELINE.md",
        "crates/agent-loop-runtime/ARCHITECTURE.md",
    ):
        p = ROOT / rel
        if not p.is_file():
            print(f"error: missing baseline doc {p}", file=sys.stderr)
            return 1

    print("check_inner_engine_docs: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
