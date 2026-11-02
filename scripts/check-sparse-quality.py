"""Evaluate the explicit synthetic gate; never equate it with general model quality."""
import argparse
import json
import math
import re
from collections import Counter
from pathlib import Path

NOISE = {"ээ", "эээ", "ээээ", "эм", "эмм", "ммм"}


def words(text):
    return re.findall(r"[^\W_]+", text.casefold())


def assess(cases, actual, *, sparse, source_preserved):
    if len(cases) != len(actual):
        raise ValueError("line count differs from corpus")
    results = []
    noise_correct = noise_total = 0
    for case, fixed in zip(cases, actual):
        original = case["text"]
        positive = case["kind"] == "noise"
        expected = [w for w in words(original) if w not in NOISE] if positive else words(original)
        preserved = words(fixed) == expected if positive else words(fixed) == words(original)
        if case.get("allow_word_reorder"):
            preserved = Counter(words(fixed)) == Counter(words(original))
        # A retained vocal pause is a usefulness miss, not a hallucinated fact.
        if positive:
            preserved = [w for w in words(fixed) if w not in NOISE] == expected
        spans = all(span.casefold() in fixed.casefold() for span in case.get("preserve", []))
        exact_required = sparse and not positive and any(
            token in words(original) for token in ("не", "ни", "нет", "нельзя", "без", "если", "кажется")
        )
        exact_required |= sparse and any(c in original for c in '?"\'«»`')
        critical_punctuation = all(original.count(c) == fixed.count(c) for c in '?!«»')
        safe = preserved and spans and critical_punctuation and (not exact_required or fixed == original)
        useful = positive and safe and not any(w in NOISE for w in words(fixed))
        noise_total += positive
        noise_correct += useful
        results.append({"id": case["id"], "kind": case["kind"], "source": original,
                        "actual": fixed, "safe": safe, "useful_noise_fix": useful})
    threshold = math.ceil(noise_total * .75)
    safety = source_preserved and all(row["safe"] for row in results)
    return {"scope": "synthetic_corpus_only", "semantic_quality_established": False,
            "safety_pass": safety, "source_preserved": source_preserved,
            "noise_correct": noise_correct, "noise_total": noise_total,
            "noise_required": threshold, "passed": safety and noise_total > 0 and noise_correct >= threshold,
            "cases": results}


def prepare_fixture(corpus, root):
    cases = corpus["cases"]
    if not cases or len({case["id"] for case in cases}) != len(cases):
        raise ValueError("nonempty corpus with unique IDs required")
    root.mkdir(parents=True, exist_ok=False)
    rows = [dict(source_id=0, start_sec=i * 10, end_sec=(i + 1) * 10, text=case["text"])
            for i, case in enumerate(cases)]
    (root / "source.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8", newline="\n")
    manifest = dict(version=1, cases=[dict(name="quality", transcript="source.jsonl", lang="ru", modes=["cleanup"])])
    (root / "manifest.json").write_text(json.dumps(manifest) + "\n", encoding="utf-8", newline="\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=Path("scripts/fixtures/sparse-edit-quality.json"))
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--output", type=Path, help="Completed text-only profiler output")
    action.add_argument("--prepare", type=Path, help="Create source/manifest in a new directory")
    args = parser.parse_args()
    corpus = json.loads(args.corpus.read_text(encoding="utf-8"))
    if args.prepare:
        prepare_fixture(corpus, args.prepare)
        print(args.prepare / "manifest.json")
        return 0
    report = json.loads((args.output / "report.json").read_text(encoding="utf-8"))
    if not report.get("success") or len(report["runs"]) != 1:
        raise ValueError("one completed successful cleanup run required")
    run = report["runs"][0]
    if run["mode"] != "cleanup" or run["repeat"] != 1:
        raise ValueError("one cleanup repeat required")
    session = args.output / "sessions" / f'{run["case"]}-cleanup-r1'
    rows = [json.loads(line) for line in (session / "transcripts/v001-profile-source.jsonl").read_text(encoding="utf-8").splitlines()]
    if [row["text"] for row in rows] != [case["text"] for case in corpus["cases"]]:
        raise ValueError("source does not match corpus")
    readable = json.loads((session / "processed.json").read_text(encoding="utf-8"))
    if readable["version_id"] != 1:
        raise ValueError("unexpected source version")
    edits = readable["edits"]
    actual = [edits.get(str(i), row["text"]) for i, row in enumerate(rows)]
    sparse = report.get("experimental_sparse_edits", False) or report.get("experimental_sparse_lines", False)
    result = assess(corpus["cases"], actual, sparse=sparse,
                    source_preserved=report["input_preserved"])
    result.update(model=report["llm_model"], sparse=sparse,
                  protocol_variant="lines" if report.get("experimental_sparse_lines") else "anchors" if sparse else "baseline",
                  wall_sec=run["wall_sec"], protocol=run.get("sparse"))
    (args.output / "quality.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f'{result["model"]} sparse={result["sparse"]}: safety={result["safety_pass"]}, '
          f'noise={result["noise_correct"]}/{result["noise_total"]}, gate={result["passed"]}')
    return 0 if result["passed"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
