# R4 · Fault/restart series, 2026-11-10

Изолированные synthetic producer → capture/planner → coordinator → настоящий
дочерний worker → Archive API. Семь режимов одного сценария:
`stop`, `disable`, `cancel`, `timeout`, `asr_error`, `missing`, `changed`.
[Код/assertions](../../../../crates/localvox-light/src/autocook/prefix_tests.rs),
[сохранённый индекс receipts](fault-series.json). ASR — явный test-only fixture,
не native модель; оба источника имеют настоящие PCM samples, второй — тишину.

## Пройдено

- Первая порция 43 s публикует читаемый текст без versions/summary. Тот же horizon
  не запускает worker повторно. Следующая порция 35 s пересекает границы файлов/VAD.
- ASR зависает (owned child timeout 2 s), явно падает, sealed файл потерян либо
  изменён. После трёх отказов job Failed; прежний Published SHA и API text сохранены,
  capture остаётся жив. После освобождения execution lease вход восстанавливается,
  явный Retry возвращает обработку; Stop даёт полный transcript receipt.
  Каждое принятое до отказа ASR окно вызывается ровно один раз. Не принятый suffix
  может повторяться, это не выдаётся за отсутствие всех повторных попыток.
- Live off завершает текущую порцию, новые ждут Stop. Cancel не останавливает
  capture; запись ещё 3 s, Stop не обходит отмену, Retry явно завершает обработку.
- Все семь режимов закончились Done, одной job и подтверждённым непустым STT,
  без summary. **1 test passed, 30.58 s**. Полные synthetic archives с PCM/seals/
  cache/versions/receipts, jobs и ASR hash log сохранены под
  `target/incremental-faults-20261110/`; индекс содержит пути и SHA отчётов.
- Дополнительная настоящая child-worker crash серия: **3 passed, 3.74 s**.
  Обрывы после result/checkpoint/transcript/manifest/best/receipt, пустой ASR,
  force/legacy и восстановление без дублирования принятых окон/версий.
  [Подробности и ограничения](prefix-worker.md).

Первый расширенный тест попытался Retry до освобождения execution lease и получил
корректный busy. Исправлено ожидание в тесте; product guard не ослаблялся.

## Остальные слои отказов

Общий core regression повторён: **448 passed / 8 ignored**, включая refusal
write/seek/sync/rename/manifest/terminal commit, empty/lost ranges, restart,
continuous VAD seams/silence/tail, cache SHA/model/recipe, codec timeout/cancel.
[Capture/codec evidence](capture-input.md), [planner](capture-planner.md),
[cache](asr-cache.md). Это контролируемая инъекция ошибок, не заполнение физического
диска и не аварийное отключение питания.

Отдельно в R3 выполнены настоящие FFmpeg/HTTP/Range/mpv проверки FLAC reader race,
побайтного PCM через шов, повреждения нужного окна без fake silence и сохранения
старого playing URL при росте: [результаты](partial-clients.md). Они не заменяются
обнаружением ignored тестов в общем прогоне. Native точность переиспользования на
всём публичном PCM проверена отдельно: [reference](native-reference.md).

Таким образом fault-серия состоит из coordinator/child/core/media слоёв; она
не утверждает, что каждый дисковый отказ воспроизведён через весь HTTP/UI путь.
Microphone/loopback, физическое power loss, Unix/слабый Mac — not-run.

## Повторить

TEMP/TMP на D:, `RUST_TEST_THREADS=2`, новый evidence каталог:

```powershell
cargo build --locked --offline -j2 -p localvox-light-asr --features asr-fixtures --bin localvox-process
$env:LOCALVOX_TEST_ASR_WORKER = (Resolve-Path target/debug/localvox-process.exe).Path
$env:LOCALVOX_TEST_PREFIX_EVIDENCE = "$PWD\target\incremental-faults-repeat"
cargo test --locked --offline -j2 -p localvox-light --lib autocook::prefix_tests -- --ignored --nocapture
cargo test --locked --offline -j2 -p localvox-light-asr --features asr-fixtures --test incremental_fixture -- --nocapture
```

Без переменной evidence временные входы удаляются после теста. Fixture CLI/engine
доступны только в явной feature-сборке; штатный release собран отдельно без неё.
