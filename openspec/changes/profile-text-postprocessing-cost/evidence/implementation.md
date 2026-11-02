# Профилирование правки текста — 2026-11-02

## Реализация инструмента

Задача 1.1 выполнена: [text-only module](../../../../crates/localvox-light-bench/src/bin/profile/text.rs)
расширяет существующий profiler. [Команды и manifest](../../../../docs/text-postprocessing-profile.md).
Продуктовые prompts, batching, модели и проверки не менялись. Native результаты —
в [отдельном свидетельстве](native-reference.md); таблица ниже их не подменяет.

| Проверка | Фактический результат | Граница |
|---|---|---|
| `cargo test --locked --offline -j 2 -p localvox-light-bench --bin localvox-profile -- --test-threads=2` | 24 passed, 2 ignored | Включая прежние WAV/proxy/process/timeouts и новые manifest/seed/env/no-overwrite/failed checkpoint |
| `LOCALVOX_PROFILE_TEST_WORKER=<release worker>`; `cargo test … text::tests::text_profile_roundtrip -- --ignored --nocapture` | 1 passed; 4 маршрута, requests 1/1/2/1, все receipts проверены | Настоящие worker/HTTP и файлы, явно подставной LLM; модели/устройства не проверяются |
| `cargo build --locked --offline --release -j 2 -p localvox-light-bench --bin localvox-profile -p localvox-light-asr --bin localvox-process` | PASS, 70 s | Два существующих warning core (cpal name, unused mut); сборка не является замером обработки |
| `rustfmt --edition 2021 --check crates/localvox-light-bench/src/bin/localvox-profile.rs` | PASS, включая дочерние модули | Форматирование |
| `openspec validate --specs --strict --no-interactive`; `validate profile-text-postprocessing-cost --type change --strict --no-interactive` | PASS | Форма main и skip_specs engineering change |
| `python scripts/check-openspec.py`; `git diff --check` | PASS: 15 capabilities, 88 requirements, 178 scenarios, 88 coverage rows | Ссылки/структура и whitespace |

Test roundtrip проверяет отсутствие исходных слов/имени speaker в JSON-отчёте,
проверку каждого артефакта и два последовательных вызова в refine-cleanup. Ошибочный
worker не выдаётся за быстрый успех; исходный JSONL сохранён, повторный output отвергается.
CI получает этот же real-worker/local-fake-LLM сценарий; удалённый CI не запускался.

## Выбор материала

Повторно используется публичная STT Podlodka #109 из
[BL-003](../../benchmark-processing-batch/evidence/queue-benchmark.md), не архив владельца.
Исходник `v001-gigaam-int8.jsonl`, SHA-256
`09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8`.
704 реплики, last_end 7146.527375 s, 186124 bytes / 103919 Unicode chars текста.

Статический расчёт текущих budgets на этих строках: refine **9** запросов,
cleanup **179** (2–6 строк в пакете). Это расчёт по алгоритму, не замер времени.
Refine и cleanup — разные последовательные проходы; при включении обоих сумма
не обязана равняться отдельным прогонам, поскольку cleanup читает исправленную версию.

Данные/условия/полный native report остаются в `target/` на D:. Пользовательские
`.env`, архив и демон не менялись. Исходный часовой инцидент на записи владельца
пока не воспроизведён. Качество отредактированного текста не считается принятым.

## Завершение измерений

Задача 2.1 выполнена: все четыре local native modes успешны, source/worker hashes
сохранены, все выходные receipts проверены. [Данные](native-results.json) содержат
исходные численные counters, состояние residency и источник полного локального report.
Refine/cleanup/chain/summary: 289.570 / 261.569 / 557.480 / 154.926 s.
Нет утверждения, что исторический часовой инцидент воспроизведён или исправлен.
Подтверждён следующий эксперимент для ручной правки — уменьшить выходную генерацию;
source/answer NER summary требует собственного более узкого профиля.

Задача 2.2 выполнена: обновлены строки PIPE-005/006/007/011 и LLM-003/004,
catalog и итоговый report. Строгая валидация main/change и check-openspec повторены:
15 capabilities, 88 requirements, 178 scenarios, 88 coverage rows, 1391 local links;
diff/форматирование без ошибок. Основные контракты и пользовательский справочник
не менялись: добавлен инженерный режим измерения, доступные действия UI/TUI прежние.
