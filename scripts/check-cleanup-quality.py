"""Evaluate explicit cleanup expectations; lexical flags on public text require review."""
import argparse
from collections import Counter
from difflib import SequenceMatcher
import hashlib
import json
from pathlib import Path
import re


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def normalize(text):
    return " ".join(text.split()).replace("ё", "е").replace("Ё", "Е")


def contains(text, fragment):
    left = r"(?<!\w)" if re.match(r"\w", fragment[:1]) else ""
    right = r"(?!\w)" if re.match(r"\w", fragment[-1:]) else ""
    return re.search(left + re.escape(fragment.casefold()) + right, text.casefold()) is not None


def assess(case, actual):
    original = case["text"]
    expected = case.get("expected", [original])
    reasons = ["missing:" + span for span in case.get("preserve", []) if not contains(actual, span)]
    for mark in "?!":
        if original.count(mark) != actual.count(mark):
            reasons.append("changed:" + mark)
    if reasons:
        status = "violation"
    elif normalize(actual) in [normalize(s) for s in expected]:
        status = "expected"
    elif normalize(actual) == normalize(original) and case["intent"] == "repair":
        status = "missed"
    else:
        status = "review"
    return dict(id=case["id"], category=case["category"], intent=case["intent"],
                source=original, actual=actual, status=status, reasons=reasons)


def corpus_cases(path):
    cases = read_json(path)["cases"]
    if not cases or len({c["id"] for c in cases}) != len(cases):
        raise ValueError("nonempty corpus with unique IDs required")
    for c in cases:
        if c["intent"] not in ("repair", "preserve") or not c["text"].strip():
            raise ValueError("invalid corpus case")
        if c["intent"] == "repair" and not c.get("expected"):
            raise ValueError("repair requires an expected result")
        for expected in c.get("expected", [c["text"]]):
            if assess(c, expected)["status"] != "expected":
                raise ValueError("expectation contradicts invariants: " + c["id"])
    return cases


def prepare(cases, root):
    root.mkdir(parents=True, exist_ok=False)
    rows = [dict(source_id=0, start_sec=i * 10, end_sec=(i + 1) * 10, text=c["text"])
            for i, c in enumerate(cases)]
    (root / "source.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows), encoding="utf-8", newline="\n")
    manifest = dict(version=1, cases=[dict(name="quality", transcript="source.jsonl", lang="ru", modes=["cleanup"])])
    (root / "manifest.json").write_text(json.dumps(manifest) + "\n", encoding="utf-8", newline="\n")


def checked_path(root, relative):
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError("artifact escapes session")
    return path


def verify_file(root, entry, *, transcript=False):
    path = checked_path(root, entry["path"])
    if entry.get("transcript", False) is not transcript:
        raise ValueError("unexpected receipt stamp type")
    # Transcript receipts hash Rust's parsed speech tuples, excluding speaker labels.
    # The worker/profiler verifies that semantic stamp. Independently require the
    # exact raw source SHA from the profiler below; do not confuse these two hashes.
    if path.stat().st_size != entry["bytes"] or (not transcript and sha256(path) != entry["sha256"]):
        raise ValueError("artifact receipt differs")
    return path


def load_runs(output):
    report = read_json(output / "report.json")
    if not all(report.get(k) is True for k in ("complete", "success", "input_preserved")):
        raise ValueError("completed successful preserved-source profile required")
    if not report.get("text_only") and report.get("scope") != "isolated_text_postprocessing":
        raise ValueError("text-only profile required")
    if report.get("experimental_sparse_edits") or report.get("experimental_sparse_lines"):
        raise ValueError("ordinary cleanup required")
    loaded = []
    seen = set()
    inputs = {entry["case"]: entry for entry in report["inputs"]}
    for run in report["runs"]:
        name, repeat = run["case"], run["repeat"]
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or type(repeat) is not int or repeat < 1:
            raise ValueError("invalid case/repeat")
        if (name, repeat) in seen or run["mode"] != "cleanup" or run["status"] != "success" or run.get("artifacts_verified") is not True:
            raise ValueError("unique verified cleanup runs required")
        seen.add((name, repeat))
        session = output / "sessions" / f"{name}-cleanup-r{repeat}"
        ledger = read_json(session / "processing.json")["artifacts"]["processed"]
        if ledger["source"] != 1 or ledger["outcome"] != "ok":
            raise ValueError("unexpected processed source/outcome")
        receipt = ledger["receipt"]
        source = verify_file(session, receipt["input"], transcript=True)
        if receipt["input"]["path"] != "transcripts/v001-profile-source.jsonl" or sha256(source) != inputs[name]["sha256"]:
            raise ValueError("unexpected source fingerprint")
        outputs = receipt["outputs"]
        if len(outputs) != 1 or outputs[0]["path"] != "processed.json":
            raise ValueError("processed receipt required")
        processed_path = verify_file(session, outputs[0])
        rows = [json.loads(s) for s in source.read_text(encoding="utf-8").splitlines()]
        processed = read_json(processed_path)
        if processed["version_id"] != 1 or len(rows) != inputs[name]["lines"]:
            raise ValueError("source version/line count differs")
        edits = processed["edits"]
        if any(not re.fullmatch(r"0|[1-9][0-9]*", k) or int(k) >= len(rows) or not isinstance(v, str) or not v.strip() for k, v in edits.items()):
            raise ValueError("invalid edit index/text")
        actual = [edits.get(str(i), row["text"]) for i, row in enumerate(rows)]
        loaded.append((run, rows, actual))
    if not loaded:
        raise ValueError("no runs")
    return report, loaded


def evaluate(cases, output):
    report, loaded = load_runs(output)
    runs = []
    for run, rows, actual in loaded:
        if [r["text"] for r in rows] != [c["text"] for c in cases]:
            raise ValueError("source differs from corpus")
        results = [assess(c, a) for c, a in zip(cases, actual)]
        counts = dict(Counter(r["status"] for r in results))
        repair = [r for r in results if r["intent"] == "repair"]
        runs.append(dict(case=run["case"], repeat=run["repeat"], wall_sec=run["wall_sec"],
                         statuses=counts, useful_expected=sum(r["status"] == "expected" for r in repair),
                         repair_total=len(repair), corrections=run.get("corrections"), cases=results))
    return dict(scope="explicit_synthetic_expectations", semantic_quality_established=False,
                model=report["llm_model"], worker_sha256=report["worker_sha256"],
                report_sha256=sha256(output / "report.json"), runs=runs)


RISK_WORDS = {"не", "ни", "нет", "нельзя", "без", "почти", "пока", "только", "кажется", "если", "кроме", "либо", "вряд", "вероятно", "наверное", "возможно"}


def tokens(text):
    return re.findall(r"[^\W_]+", text.casefold())


def risk_flags(original, actual):
    before, after = Counter(tokens(original)), Counter(tokens(actual))
    flags = []
    if any(before[w] != after[w] for w in RISK_WORDS):
        flags.append("qualifier")
    if re.findall(r"\d+(?:[.,:/–-]\d+)*", original) != re.findall(r"\d+(?:[.,:/–-]\d+)*", actual):
        flags.append("number")
    if re.findall(r"«[^»]*»|\"[^\"]*\"|`[^`]*`", original) != re.findall(r"«[^»]*»|\"[^\"]*\"|`[^`]*`", actual):
        flags.append("literal")
    if any(original.count(c) != actual.count(c) for c in "?!"):
        flags.append("question_emphasis")
    if before != after:
        flags.append("lexical")
    return flags


def select_review(changes, limit=24):
    """All number flags, four quantiles per critical flag, then low-flag controls."""
    chosen = {c["index"] for c in changes if "number" in c["flags"]}
    for flag in ("qualifier", "literal", "question_emphasis"):
        group = [c["index"] for c in changes if flag in c["flags"]]
        if group:
            chosen.update(group[round(i * (len(group) - 1) / 3)] for i in range(4))
    controls = [c["index"] for c in changes if not (set(c["flags"]) - {"lexical"})]
    remaining = max(0, limit - len(chosen))
    if controls and remaining:
        chosen.update(controls[round(i * (len(controls) - 1) / max(1, remaining - 1))] for i in range(remaining))
    return sorted(chosen)


def adjacent_overlap_flags(rows, actual):
    flags = []
    for i, (row, fixed) in enumerate(zip(rows, actual)):
        if row["text"] == fixed:
            continue
        original = " ".join(row["text"].casefold().split())
        for side, j in (("previous", i - 1), ("next", i + 1)):
            if not 0 <= j < len(rows):
                continue
            neighbor = rows[j]["text"].casefold().split()
            match = SequenceMatcher(a=neighbor, b=fixed.casefold().split(), autojunk=False).find_longest_match()
            phrase = " ".join(neighbor[match.a:match.a + match.size])
            if match.size >= 8 and phrase not in original:
                flags.append(dict(index=i, neighbor=side, matched_words=match.size))
    return flags


def scan(output):
    report, loaded = load_runs(output)
    runs = []
    for run, rows, actual in loaded:
        changes = []
        for i, (row, fixed) in enumerate(zip(rows, actual)):
            if row["text"] != fixed:
                changes.append(dict(index=i, start_sec=row["start_sec"], source=row["text"], actual=fixed,
                                    flags=risk_flags(row["text"], fixed),
                                    previous=rows[i - 1]["text"] if i else None,
                                    next=rows[i + 1]["text"] if i + 1 < len(rows) else None))
        selected = select_review(changes)
        overlaps = adjacent_overlap_flags(rows, actual)
        runs.append(dict(case=run["case"], repeat=run["repeat"], total=len(rows), changed=len(changes),
                         flag_counts=dict(Counter(f for c in changes for f in c["flags"])),
                         review_selection="all numbers; four quantiles per qualifier/literal/question flag; evenly spaced low-flag controls to 24",
                         selected_indices=selected, adjacent_overlap_flags=overlaps,
                         supplemental_review_indices=sorted({f["index"] for f in overlaps} - set(selected)),
                         changes=changes))
    return dict(scope="lexical_risk_scan_not_semantic_verdict", model=report["llm_model"],
                semantic_quality_established=False, report_sha256=sha256(output / "report.json"), runs=runs)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--corpus", type=Path, default=Path("scripts/fixtures/cleanup-quality-ru.json"))
    action = p.add_mutually_exclusive_group(required=True)
    action.add_argument("--prepare", type=Path)
    action.add_argument("--output", type=Path)
    action.add_argument("--scan", type=Path, help="Public profile risk flags, not a quality score")
    p.add_argument("--write", type=Path, help="New result file; includes text, keep private outputs out of git")
    args = p.parse_args()
    if args.prepare:
        prepare(corpus_cases(args.corpus), args.prepare)
        print(args.prepare / "manifest.json")
        return 0
    result = scan(args.scan) if args.scan else evaluate(corpus_cases(args.corpus), args.output)
    if args.output:
        result["corpus_sha256"] = sha256(args.corpus)
    if args.write:
        with args.write.open("x", encoding="utf-8", newline="\n") as out:
            json.dump(result, out, ensure_ascii=False, indent=2)
            out.write("\n")
    for run in result["runs"]:
        print(result["model"], run["repeat"], run.get("statuses", run.get("flag_counts")))
    if args.scan:
        return 0
    return 2 if any(r["statuses"].get("violation") or r["statuses"].get("review") or r["useful_expected"] != r["repair_total"] for r in result["runs"]) else 0


if __name__ == "__main__":
    raise SystemExit(main())
