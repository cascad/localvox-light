"""Engineering CPU-only SAGE test; no application settings, worker guards or downloads."""
import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
import platform
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New JSON file; includes corpus text")
    parser.add_argument("--corpus", type=Path, action="append")
    parser.add_argument("--threads", type=int, default=2, choices=range(1, 5))
    parser.add_argument("--repeats", type=int, default=3, choices=range(1, 11))
    args = parser.parse_args()
    if not args.model_dir.is_dir():
        parser.error("model directory must already exist; this tool never downloads")
    spec = importlib.util.spec_from_file_location("quality", Path(__file__).with_name("check-cleanup-quality.py"))
    quality = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(quality)
    corpora = args.corpus or [Path(__file__).parent / "fixtures" / name for name in
                             ("cleanup-quality-ru.json", "cleanup-boundaries-ru.json")]
    prepared = [(path, quality.corpus_cases(path)) for path in corpora]
    report = dict(scope="raw_sage_cpu_no_worker_guards", complete=False, success=False,
                  semantic_quality_established=False, production_guards_executed=False,
                  device="cpu", threads=args.threads, python=platform.python_version(),
                  os=platform.platform(), processor=platform.processor(),
                  weights_sha256=quality.sha256(args.model_dir / "model.safetensors"),
                  generation=dict(do_sample=False, num_beams=1, use_cache=True,
                                  max_new_tokens="min(256, max(16, 2*input_tokens+16))"),
                  runs=[])
    with args.output.open("x", encoding="utf-8", newline="\n") as out:
        json.dump(report, out)

    def checkpoint():
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")

    try:
        started = time.perf_counter()
        import torch
        import transformers
        from transformers import AutoModelForSeq2SeqLM, AutoTokenizer
        report["imports_sec"] = time.perf_counter() - started
        report.update(torch=torch.__version__, transformers=transformers.__version__)
        torch.set_num_threads(args.threads)
        torch.set_num_interop_threads(1)
        started = time.perf_counter()
        tokenizer = AutoTokenizer.from_pretrained(args.model_dir, local_files_only=True, trust_remote_code=False)
        model = AutoModelForSeq2SeqLM.from_pretrained(args.model_dir, local_files_only=True,
                                                     trust_remote_code=False, use_safetensors=True)
        model.to("cpu").eval()
        report["model_load_sec"] = time.perf_counter() - started
        report["parameter_bytes_not_peak_rss"] = sum(p.numel() * p.element_size() for p in model.parameters())
        for path, cases in prepared:
            for repeat in range(1, args.repeats + 1):
                started = time.perf_counter()
                rows = []
                for case in cases:
                    case_started = time.perf_counter()
                    encoded = tokenizer(case["text"], truncation=False, return_tensors="pt")
                    input_tokens = encoded["input_ids"].shape[1]
                    if input_tokens > 512:
                        raise ValueError("input exceeds experimental limit; truncation prohibited")
                    generation_started = time.perf_counter()
                    with torch.inference_mode():
                        output = model.generate(**encoded, do_sample=False, num_beams=1, use_cache=True,
                                                max_new_tokens=min(256, max(16, 2 * input_tokens + 16)))
                    generation_sec = time.perf_counter() - generation_started
                    actual = tokenizer.decode(output[0], skip_special_tokens=True)
                    row = quality.assess(case, actual)
                    eos = model.generation_config.eos_token_id
                    eos_ids = eos if isinstance(eos, list) else [eos]
                    terminated = int(output[0, -1]) in eos_ids
                    row.update(wall_sec=time.perf_counter() - case_started, generation_sec=generation_sec,
                               input_tokens=input_tokens, output_tokens=output.shape[1] - 1,
                               terminated_with_eos=terminated)
                    if not terminated:
                        row["status"] = "review"
                        row["reasons"].append("incomplete_generation")
                    rows.append(row)
                scoped = [r for r in rows if r["category"] in ("format", "asr-repair")]
                report["runs"].append(dict(corpus=path.name, corpus_sha256=quality.sha256(path),
                    repeat=repeat, wall_sec=time.perf_counter() - started,
                    statuses=dict(Counter(r["status"] for r in rows)),
                    format_asr_expected=sum(r["status"] == "expected" for r in scoped),
                    format_asr_total=len(scoped), cases=rows))
                checkpoint()
                print(path.name, repeat, report["runs"][-1]["statuses"], flush=True)
        report["complete"] = True
        report["success"] = True
        checkpoint()
        return 0
    except Exception as error:
        report["failure_type"] = type(error).__name__
        checkpoint()
        raise


if __name__ == "__main__":
    raise SystemExit(main())
