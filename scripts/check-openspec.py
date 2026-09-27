"""Check the accepted OpenSpec baseline's structural traceability (stdlib only).

Does not infer code correctness, run models, or promote evidence to acceptance.
Run alongside: openspec validate --specs --strict --no-interactive
"""

import argparse
from collections import Counter
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]
REQUIREMENT = re.compile(r"^### Requirement: ([A-Z]+(?:-[A-Z]+)*-\d+) (.+)$", re.M)
LINK = re.compile(r"\[[^\]\n]+\]\((<[^>]+>|[^)\s]+)\)")


def slug(text):
    text = re.sub(r"<[^>]+>", "", text).lower().strip()
    return "".join(c for c in text if c.isalnum() or c in " _-").replace(" ", "-")


def anchors(text):
    used = Counter()
    result = set(re.findall(r'<a\s+(?:id|name)=["\']([^"\']+)["\']', text))
    for title in re.findall(r"^#{1,6} (.+?)\s*#*\s*$", without_fences(text), re.M):
        base = slug(title)
        name = base if not used[base] else f"{base}-{used[base]}"
        used[base] += 1
        result.add(name)
    return result


def without_fences(text):
    return re.sub(r"^```[^\n]*\n[\s\S]*?^```\s*$", "", text, flags=re.M)


def check(root):
    errors = []
    specs = sorted((root / "openspec/specs").glob("*/spec.md"))
    coverage_path = root / "openspec/evidence/coverage.md"
    if not specs or not coverage_path.is_file():
        return ["Missing main specs or evidence/coverage.md"], {}
    coverage = coverage_path.read_text(encoding="utf-8")
    review = (root / "openspec/review.md").read_text(encoding="utf-8")
    backlog = (root / "openspec/backlog.md").read_text(encoding="utf-8")
    for kind, registry in (("RV", review), ("BL", backlog)):
        for identifier in set(re.findall(rf"\b{kind}-\d+\b", coverage)):
            if identifier not in registry:
                errors.append(f"Unknown coverage issue: {identifier}")
    rows = re.findall(r"^\| \[([A-Z]+(?:-[A-Z]+)*-\d+)\]\([^\n]+$", coverage, re.M)
    expected = []
    scenarios = 0
    for file in specs:
        text = file.read_text(encoding="utf-8")
        if "Статус контракта: **accepted**" not in text:
            errors.append(f"{file.relative_to(root)}: missing accepted contract status")
        matches = list(REQUIREMENT.finditer(text))
        if not matches:
            errors.append(f"{file}: no requirement IDs")
        for index, match in enumerate(matches):
            rid = match[1]
            expected.append(rid)
            end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
            block = re.split(r"^## ", text[match.end():end], maxsplit=1, flags=re.M)[0]
            if not re.search(r"\b(MUST|SHALL)\b", block):
                errors.append(f"{rid}: missing normative statement")
            source_links = [target.strip("<>").split("#")[0] for target in LINK.findall(block)]
            source_links = [p for p in source_links if p.startswith("../../../") and
                            p.endswith((".rs", ".ts", ".tsx", ".py", ".ps1", ".sh", ".json", ".toml"))]
            if not source_links:
                errors.append(f"{rid}: missing concrete code entry point")
            cases = re.split(r"^#### Scenario: ", block, flags=re.M)[1:]
            scenarios += len(cases)
            if not cases:
                errors.append(f"{rid}: no scenarios")
            for case in cases:
                if "**WHEN**" not in case or "**THEN**" not in case:
                    errors.append(f"{rid}: incomplete WHEN/THEN: {case.splitlines()[0]}")
    for rid, count in Counter(expected).items():
        if count != 1:
            errors.append(f"Duplicate requirement: {rid}")
    for rid, count in Counter(rows).items():
        if count != 1:
            errors.append(f"Duplicate coverage row: {rid}")
    for rid in sorted(set(expected) - set(rows)):
        errors.append(f"Missing coverage row: {rid}")
    for rid in sorted(set(rows) - set(expected)):
        errors.append(f"Orphan coverage row: {rid}")
    for row in coverage.splitlines():
        if re.match(r"^\| \[[A-Z]+(?:-[A-Z]+)*-\d+\]", row):
            cells = row.strip("|").split("|")
            if len(cells) != 4 or any(not c.strip() for c in cells):
                errors.append("Coverage row needs requirement, code, evidence, limits: " + row[:100])
            if not any(tag in row for tag in ("code-reviewed", "test-run", "runtime-mock")):
                errors.append("Missing evidence classification: " + row[:100])
    # Test references must still point at a real function; execution is a dated human record.
    rust = "\n".join(p.read_text(encoding="utf-8") for p in (root / "crates").rglob("*.rs"))
    for test in re.findall(r"test:([a-z][a-z0-9_]+)", coverage):
        if not re.search(r"\bfn\s+" + re.escape(test) + r"\s*\(", rust):
            errors.append(f"Missing referenced test function: {test}")
    docs = sorted((root / "openspec").rglob("*.md"))
    docs += [root / "README.md", root / "AGENTS.md", root / "docs/README.md"]
    target_anchors = {}
    links = 0
    for file in docs:
        for dest in LINK.findall(without_fences(file.read_text(encoding="utf-8"))):
            dest = unquote(dest.strip("<>"))
            if urlsplit(dest).scheme:
                continue
            path, _, anchor = dest.partition("#")
            target = (file.parent / path).resolve() if path else file.resolve()
            links += 1
            if not target.exists():
                errors.append(f"{file.relative_to(root)}: broken link {dest}")
            elif anchor and target.suffix == ".md":
                if target not in target_anchors:
                    target_anchors[target] = anchors(target.read_text(encoding="utf-8"))
                if anchor not in target_anchors[target]:
                    errors.append(f"{file.relative_to(root)}: missing anchor {dest}")
    return errors, dict(capabilities=len(specs), requirements=len(expected),
                        scenarios=scenarios, coverage_rows=len(rows), local_links=links)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT,
                        help="Repository root (also allows isolated checker fault tests)")
    args = parser.parse_args()
    errors, counts = check(args.root.resolve())
    for error in errors:
        print("ERROR:", error, file=sys.stderr)
    print(("FAIL" if errors else "PASS") + ": " + ", ".join(f"{k}={v}" for k, v in counts.items()))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
