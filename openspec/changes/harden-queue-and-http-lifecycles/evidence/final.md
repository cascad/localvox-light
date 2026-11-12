# Финальная проверка двух срезов · 2026-11-12

Queue этап закоммичен отдельно: `fa6ff32`. Воспроизведение и исправление —
[queue](queue.md). HTTP resource lifetime — [http](http.md).
Объём ограничен тремя задачами; новые продуктовые требования не вводились.

## Test-run

Windows, release, offline/locked, cargo jobs=2, RUST_TEST_THREADS=2,
INSTA_UPDATE=no. TEMP/TMP/build/package outputs в target/ на D:.

| Запуск | Результат |
|---|---|
| `cargo test --release --locked --offline --jobs 2 -p localvox-light-core --lib` | 440 passed / 8 ignored; включает 78 jobs tests |
| `cargo test --release --locked --offline --jobs 2 -p localvox-light-api` | 86 unit passed / 8 ignored, 3 integration passed; bin/doc targets по 0 tests |
| `cargo test --release --locked --offline --jobs 2 -p localvox-light --lib --bin localvox-light --test setup` | 16 lib + 1 main + 10 setup passed; 11 lib helpers/devices ignored |
| `cargo build --release --locked --offline --jobs 2 -p localvox-light` | passed |
| `python scripts/package-release.py --target x86_64-pc-windows-msvc --source target/modular-build/release --output target/setup-delivery-check` | complete runtime archive и native startup smoke passed |

Итого 556 passed без двойного подсчёта целевых jobs/budget прогонов. Ignored
helpers/fixtures/hardware не выданы за общий executed coverage; штатные parent
tests запускают свои дочерние fixtures отдельно.

API включает ошибки записи settings до apply_live, реальный journal/client HTTP,
semantic overload guard и hybrid dedup. Полный core включает queue/storage/
processing/workflow/legacy/cancel/retry. App повторяет outcome/startup recovery,
отключённую постобработку, resident worker lifetime и setup recheck/no-op/errors.

Scoped rustfmt/diff, strict main/change/check-openspec и actionlint затронутого
workflow — PASS. Strict main: 15 passed / 0 failed; check-openspec: 15 capabilities /
88 requirements / 188 scenarios / 88 coverage rows / 1944 links. Budget regression добавлен в существующий
interface CI job. Новый hosted Actions запуск не выполнялся.

## Limits

ASR model/recipes/worker protocol, UI/TUI source, HTTP DTO/authorization/body limit,
disk schema и пользовательские сценарии не менялись. User-guide сохраняет прежние
US-ID/действия; оснований переписывать его в этом срезе нет.

Рабочие daemon/archive/.env/модели не использовались для записи. Native Mac,
микрофон/звук, новый performance gate, process abort/power-loss и настоящий OS
thread exhaustion не проверялись. BL-006 и старые RV/gaps этим refactor не закрыты.
Main specs не sync/archive; push не выполняется. Обе заявленные практические
работы завершены, дополнительных refactor задач в этот change нет.
