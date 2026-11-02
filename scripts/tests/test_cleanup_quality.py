import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("cleanup_quality", Path(__file__).resolve().parents[1] / "check-cleanup-quality.py")
quality = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quality)


class CleanupQuality(unittest.TestCase):
    def test_every_corpus_expectation_satisfies_its_own_invariants(self):
        cases = quality.corpus_cases(Path(__file__).resolve().parents[1] / "fixtures/cleanup-quality-ru.json")
        self.assertEqual(len(cases), 72)
        self.assertEqual(sum(c["intent"] == "repair" for c in cases), 24)

    def test_lost_qualifier_is_a_violation_even_if_most_words_survive(self):
        c = dict(id="almost", category="meaning", intent="preserve", text="Проект почти готов.", preserve=["почти"])
        self.assertEqual(quality.assess(c, "Проект готов.")["status"], "violation")

    def test_negation_requires_a_word_boundary(self):
        self.assertFalse(quality.contains("Письмо неподтверждено.", "не"))
        self.assertTrue(quality.contains("Письмо не подтверждено.", "не"))

    def test_noop_is_a_miss_and_other_rephrasing_requires_review(self):
        c = dict(id="noise", category="vocal", intent="repair", text="Эээ, проект готов.", expected=["Проект готов."])
        self.assertEqual(quality.assess(c, c["text"])["status"], "missed")
        self.assertEqual(quality.assess(c, "Проект готов.")["status"], "expected")
        self.assertEqual(quality.assess(c, "Проект полностью завершён.")["status"], "review")

    def test_known_equivalent_order_is_allowed(self):
        c = dict(id="order", category="negation", intent="preserve", text="Без согласования запуск запрещён.",
                 expected=["Без согласования запуск запрещён.", "Запуск без согласования запрещён."], preserve=["без согласования", "запрещён"])
        self.assertEqual(quality.assess(c, c["expected"][1])["status"], "expected")

    def test_number_and_literal_changes_violate_explicit_expectations(self):
        for original, span, changed in [("Ставка 1,5 процента.", "1,5", "Ставка 15 процентов."),
                                        ("Он сказал «эээ».", "«эээ»", "Он сказал «э»."),
                                        ("Казнить нельзя, помиловать.", "нельзя, помиловать", "Казнить, нельзя помиловать.")]:
            c = dict(id="critical", category="meaning", intent="preserve", text=original, preserve=[span])
            self.assertEqual(quality.assess(c, changed)["status"], "violation")

    def test_statement_cannot_become_a_question(self):
        c = dict(id="statement", category="meaning", intent="preserve", text="Проект готов.")
        self.assertEqual(quality.assess(c, "Проект готов?")["status"], "violation")

    def test_lexical_flags_alone_do_not_claim_an_error(self):
        self.assertEqual(quality.risk_flags("эээ, проект готов", "Проект готов."), ["lexical"])
        self.assertIn("qualifier", quality.risk_flags("Проект почти готов.", "Проект готов."))

    def test_review_selection_keeps_every_number_and_spans_flagged_and_control_rows(self):
        changes = [dict(index=i, flags=["qualifier"] if i % 2 else ["lexical"]) for i in range(100)]
        changes[42]["flags"] = ["number"]
        selected = quality.select_review(changes)
        self.assertEqual(len(selected), 24)
        self.assertTrue({0, 1, 42, 98, 99}.issubset(selected))
        self.assertEqual(selected, quality.select_review(changes))

    def test_neighbor_copy_is_flagged_without_calling_shared_source_words_an_error(self):
        rows = [dict(text="Начало фразы."), dict(text="Это продолжение длинной фразы с восемью разными словами здесь.")]
        actual = [rows[0]["text"] + " " + rows[1]["text"], rows[1]["text"]]
        self.assertEqual(quality.adjacent_overlap_flags(rows, actual), [dict(index=0, neighbor="next", matched_words=9)])
        self.assertEqual(quality.adjacent_overlap_flags(rows, [r["text"] for r in rows]), [])
        shared = [dict(text=rows[1]["text"]), rows[1]]
        self.assertEqual(quality.adjacent_overlap_flags(shared, ["Да. " + rows[1]["text"], rows[1]["text"]]), [])

    def test_prepare_never_replaces_an_existing_directory(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "new"
            quality.prepare([dict(text="Пример.")], path)
            with self.assertRaises(FileExistsError):
                quality.prepare([dict(text="Другой пример.")], path)

    def fixture(self, root):
        session = root / "sessions/quality-cleanup-r1"
        (session / "transcripts").mkdir(parents=True)
        source = session / "transcripts/v001-profile-source.jsonl"
        source.write_text(json.dumps(dict(text="Эээ, проект готов.", source_id=0, start_sec=0, end_sec=10)) + "\n", encoding="utf-8")
        processed = session / "processed.json"
        processed.write_text(json.dumps(dict(version_id=1, edits={"0": "Проект готов."})), encoding="utf-8")
        def entry(path):
            return dict(path=path.relative_to(session).as_posix(), bytes=path.stat().st_size,
                        sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        ledger = dict(artifacts=dict(processed=dict(source=1, outcome="ok", receipt=dict(input=entry(source), outputs=[entry(processed)]))))
        ledger["artifacts"]["processed"]["receipt"]["input"].update(transcript=True, sha256="f" * 64)
        (session / "processing.json").write_text(json.dumps(ledger), encoding="utf-8")
        report = dict(complete=True, success=True, input_preserved=True, scope="isolated_text_postprocessing",
                      llm_model="fixture", worker_sha256="fixture", inputs=[dict(case="quality", lines=1, sha256=entry(source)["sha256"])],
                      runs=[dict(case="quality", repeat=1, mode="cleanup", status="success", artifacts_verified=True, wall_sec=.1)])
        (root / "report.json").write_text(json.dumps(report), encoding="utf-8")
        return session, report, ledger

    def test_verified_real_artifact_shape_is_evaluated_and_tampering_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            session, _, _ = self.fixture(root)
            c = dict(id="one", category="vocal", intent="repair", text="Эээ, проект готов.", expected=["Проект готов."])
            self.assertEqual(quality.evaluate([c], root)["runs"][0]["useful_expected"], 1)
            (session / "processed.json").write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "receipt"):
                quality.load_runs(root)

    def test_wrong_version_and_out_of_bounds_edit_are_rejected_even_with_matching_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            session, _, ledger = self.fixture(root)
            for bad in [dict(version_id=2, edits={}), dict(version_id=1, edits={"1":"invented"})]:
                p = session / "processed.json"
                p.write_text(json.dumps(bad), encoding="utf-8")
                ledger["artifacts"]["processed"]["receipt"]["outputs"][0].update(bytes=p.stat().st_size, sha256=quality.sha256(p))
                (session / "processing.json").write_text(json.dumps(ledger), encoding="utf-8")
                with self.assertRaises(ValueError):
                    quality.load_runs(root)

    def test_raw_source_hash_is_checked_separately_from_the_semantic_transcript_stamp(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            session, _, _ = self.fixture(root)
            source = session / "transcripts/v001-profile-source.jsonl"
            data = source.read_text(encoding="utf-8")
            source.write_text(data.replace("0", "1", 1), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "source fingerprint"):
                quality.load_runs(root)

    def test_partial_sparse_traversal_and_duplicate_runs_are_not_accepted_as_quality_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _, report, _ = self.fixture(root)
            for key, value in [("complete", False), ("experimental_sparse_lines", True),
                               ("runs", [dict(report["runs"][0], case="../../outside")]),
                               ("runs", [report["runs"][0], report["runs"][0]])]:
                bad = copy.deepcopy(report)
                bad[key] = value
                (root / "report.json").write_text(json.dumps(bad), encoding="utf-8")
                with self.assertRaises(ValueError):
                    quality.load_runs(root)


if __name__ == "__main__":
    unittest.main()
