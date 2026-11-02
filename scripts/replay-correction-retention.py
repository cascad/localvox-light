"""Replay saved proposals through the compiled guard; never calls a model or writes sessions."""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--checker", type=Path, required=True)
    p.add_argument("--profile", type=Path, action="append", default=[])
    p.add_argument("--sage", type=Path, action="append", default=[])
    p.add_argument("--output", type=Path, required=True, help="New content-free report")
    args = p.parse_args()
    if args.output.exists():
        p.error("output must be new")
    spec = importlib.util.spec_from_file_location("quality", Path(__file__).with_name("check-cleanup-quality.py"))
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)
    corpora = [q.corpus_cases(Path(__file__).parent / "fixtures" / name)
               for name in ("cleanup-quality-ru.json", "cleanup-boundaries-ru.json")]
    groups, proposals = [], []
    for output in args.profile:
        report, loaded = q.load_runs(output)
        for run, rows, actual in loaded:
            pairs = [(row["text"], fixed) for row, fixed in zip(rows, actual)]
            groups.append(dict(origin="saved_worker_result", model=report["llm_model"],
                               report_sha256=q.sha256(output / "report.json"), repeat=run["repeat"], pairs=pairs))
            proposals.extend(pairs)
    for path in args.sage:
        report = q.read_json(path)
        if not report.get("complete") or not report.get("success"):
            raise ValueError("complete SAGE report required")
        for run in report["runs"]:
            pairs = [(row["source"], row["actual"]) for row in run["cases"]]
            groups.append(dict(origin="saved_raw_sage_no_worker", model="sage-fredt5-distilled-95m",
                               report_sha256=q.sha256(path), repeat=run["repeat"], pairs=pairs))
            proposals.extend(pairs)
    if not proposals:
        p.error("at least one completed profile/SAGE report required")
    payload = "".join(json.dumps(dict(source=src, candidate=fixed), ensure_ascii=False) + "\n" for src, fixed in proposals)
    completed = subprocess.run([str(args.checker.resolve())], input=payload.encode("utf-8"),
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120, check=True)
    decisions = [json.loads(s) for s in completed.stdout.decode("utf-8").splitlines()]
    if len(decisions) != len(proposals):
        raise ValueError("one guard decision per proposal required")
    offset = 0
    results = []
    for group in groups:
        pairs = group.pop("pairs")
        chosen = decisions[offset:offset + len(pairs)]
        offset += len(pairs)
        def stamp(text):
            return hashlib.sha256(text.encode("utf-8")).hexdigest()
        changed = [(i, s, f, decision) for i, ((s, f), decision) in enumerate(zip(pairs, chosen)) if s != f]
        times = sorted(r["elapsed_ns"] for r in chosen)
        group.update(lines=len(pairs), changed_proposals=len(changed),
            accepted_changes=sum(d["accepted"] for _, _, _, d in changed),
            reasons=dict(Counter(d["reason"] for _, _, _, d in changed if not d["accepted"])),
            guard_total_sec=sum(times) / 1e9, guard_p95_ns=times[(len(times) - 1) * 95 // 100], guard_max_ns=max(times),
            changed=[dict(index=i,source_sha256=stamp(s),candidate_sha256=stamp(f),**d) for i,s,f,d in changed])
        source_text = [s for s, _ in pairs]
        corpus = next((cs for cs in corpora if [c["text"] for c in cs] == source_text), None)
        if corpus:
            before = [q.assess(c, f) for c, (_, f) in zip(corpus, pairs)]
            after = [q.assess(c, f if d["accepted"] else s) for c,(s,f),d in zip(corpus,pairs,chosen)]
            group["explicit_corpus"] = dict(
                before_statuses=dict(Counter(r["status"] for r in before)),
                after_statuses=dict(Counter(r["status"] for r in after)),
                useful_before=sum(r["intent"] == "repair" and r["status"] == "expected" for r in before),
                useful_after=sum(r["intent"] == "repair" and r["status"] == "expected" for r in after))
        results.append(group)
    report = dict(scope="saved_proposal_replay_not_new_model_calls", semantic_quality_established=False,
                  checker_sha256=q.sha256(args.checker), runner_sha256=q.sha256(Path(__file__)),
                  evaluator_sha256=q.sha256(Path(__file__).with_name("check-cleanup-quality.py")), groups=results)
    with args.output.open("x", encoding="utf-8", newline="\n") as out:
        json.dump(report, out, ensure_ascii=False, indent=2)
        out.write("\n")
    for r in results:
        print(r["model"],r["lines"],"accepted",r["accepted_changes"],"/",r["changed_proposals"],r.get("explicit_corpus",{}))


if __name__ == "__main__":
    main()
