import importlib.util
import unittest
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location("sparse_quality", Path(__file__).resolve().parents[1] / "check-sparse-quality.py")
quality = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quality)


class QualityGate(unittest.TestCase):
    def test_preparing_a_fixture_never_replaces_an_existing_directory(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "fixture"
            corpus = {"cases": [{"id":"one","text":"Пример."}]}
            quality.prepare_fixture(corpus,path)
            source = (path / "source.jsonl").read_bytes()
            with self.assertRaises(FileExistsError):
                quality.prepare_fixture(corpus,path)
            self.assertEqual((path / "source.jsonl").read_bytes(),source)

    def test_a_fast_noop_is_not_a_quality_success(self):
        cases = [{"id": "one", "kind": "noise", "text": "Эээ, проект готов."}]
        result = quality.assess(cases, [cases[0]["text"]], sparse=True, source_preserved=True)
        self.assertTrue(result["safety_pass"])
        self.assertFalse(result["passed"])

    def test_useful_changes_do_not_excuse_a_changed_fact(self):
        cases = [{"id": "one", "kind": "noise", "text": "Эээ, проект готов."},
                 {"id": "fact", "kind": "protected", "text": "Оплата не подтверждена."}]
        result = quality.assess(cases, ["Проект готов.", "Оплата подтверждена."], sparse=True, source_preserved=True)
        self.assertEqual(result["noise_correct"], 1)
        self.assertFalse(result["passed"])

    def test_expected_edits_with_preserved_source_pass_only_the_named_corpus(self):
        cases = [{"id": "one", "kind": "noise", "text": "Эээ, проект готов."}]
        result = quality.assess(cases, ["Проект готов."], sparse=True, source_preserved=True)
        self.assertTrue(result["passed"])
        self.assertFalse(result["semantic_quality_established"])
        self.assertFalse(quality.assess(cases, ["Проект готов."], sparse=True, source_preserved=False)["passed"])

    def test_decimal_punctuation_is_checked_beyond_word_identity(self):
        cases = [{"id": "decimal", "kind": "protected", "text": "Ставка 1,5 процента.", "preserve": ["1,5"]}]
        self.assertFalse(quality.assess(cases, ["Ставка 1.5 процента."], sparse=True, source_preserved=True)["passed"])

    def test_a_question_cannot_replace_an_assertion_without_changing_words(self):
        cases = [{"id":"assertion","kind":"protected","text":"Проект готов."}]
        self.assertFalse(quality.assess(cases,["Проект готов?"],sparse=True,source_preserved=True)["safety_pass"])

    def test_an_explicitly_equivalent_word_order_is_not_counted_as_lost_content(self):
        cases=[{"id":"order","kind":"protected","text":"Без согласования запуск запрещён.","allow_word_reorder":True}]
        result=quality.assess(cases,["Запуск без согласования запрещён."],sparse=False,source_preserved=True)
        self.assertTrue(result["safety_pass"])


if __name__ == "__main__":
    unittest.main()
