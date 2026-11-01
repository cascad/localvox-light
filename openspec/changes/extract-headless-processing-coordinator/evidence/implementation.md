# Библиотечный координатор и headless стенд, 2026-11-01

Изменение инструментов/структуры, `skip_specs: true`. PIPE-002/008/009/010/011,
форматы jobs/journal и поведение UI/TUI сохраняются. Установленный daemon,
пользовательский архив и устройства не запускались и не менялись.

## Реализация

- `crates/localvox-light/src/lib.rs` экспортирует прежний `autocook.rs`; main
  использует библиотеку. Файл и реализация координатора не копировались/не переносились.
  Его прежние 11 tests + один ignored профиль теперь выполняются в lib, один launcher
  test остаётся в binary. Три новых observer tests добавлены отдельно.
- `autocook/observer.rs`: optional bounded sync channel, `try_send` после успешных
  start/finish/failure commits под queue mutex. Отправляются часы принятого Job,
  phase/execution/state и bool ошибки, без raw error/текста. Consumer/overflow не
  меняют исход обработки; потерянные события считаются. Обычный spawn: observer=None.
- `localvox-queue-profile` запускает тот же coordinator с marker/recording/worker/
  session-path/queue guards и exclusive workspace claim. Не читает `.env`, не
  запускает capture/API. Worker определяется прежним sibling resolver; переданный
  путь обязан совпадать. Checkpoint явно неполный, final публикуется после stop/join.
- Queue wait/phase wall вычисляются из сохранённых границ, а не времени polling;
  missing/reversed/invalid = null. Complete отдельно от all_success, потери событий
  делают замер неполным. Отчёт не содержит last_error или документы встреч.
- `localvox-profile` помечает только вновь созданный output. CI запускает root lib,
  binary, launcher checks, оба профилировщика и реальный model-free queue smoke.

## Выполненные проверки

PowerShell, Windows, `TEMP/TMP=target/test-temp` на D:, `--locked -j 2`, tests threads=2.

| Команда / область | Результат |
|---|---|
| `cargo test -p localvox-light --lib --bin localvox-light` | lib 14 passed, 1 ignored (отдельный синтетический профиль); bin 1 passed |
| `cargo test -p localvox-light-bench --bin localvox-profile --bin localvox-queue-profile` | phase profiler 15 passed, 1 ignored fixture; queue profiler 4 passed, 1 ignored runtime |
| Queue runtime `--ignored --exact tests::real_coordinator_empty_timeout_and_preflight_failure_are_bounded` после сборки queue binary | 1 passed: настоящий worker protocol/empty ready→terminal; timeout/кооперативный stop с сохранённым заданием; несовместимый sibling отклонён без запуска фазы |
| `cargo test -p localvox-light-core jobs:: --lib` | 60 passed |
| `cargo test -p localvox-light-core workflow:: --lib` | 11 passed |

Observer tests: совпадение queued/started/ended/execution с сохранёнными Job, оба
accepted/failed исхода, отсутствие error content, channel overflow без ожидания
consumer; на Windows denied replacement jobs.json не публикует исход до принятого
commit. Сохранились прежние startup/outcome persistence и discovery concurrency tests.

Queue guards: отсутствующий/неверный marker, recording marker (включая пустой),
нечитаемая очередь, иной worker, существующий report, unsafe/running session и
двойной workspace claim. Clock tests сохраняют null и настоящий zero.

`launcher_modes --ignored`: 1 passed; `cargo check -p localvox-light
--no-default-features`: passed. Strict main specs: 15 passed; strict change: valid;
`check-openspec.py`: 15 capability, 86 requirements, 168 scenarios, 86 coverage rows.
Проверки rustfmt затронутых файлов и diff выполнены перед коммитом.
Два известных core warnings с ONNX
(`cpal::Device::name`, unused_mut engine.rs) относятся к прежнему коду.

## Границы

Это готовый headless вход, не полный benchmark BL-003: нет новой оценки throughput
длинной речи через очередь, RAM дерева/GPU/отдельного LLM или контролируемого
cold/warm. Качество summary/полнота текста не проверены. Физические микрофон,
system audio и приёмка UI/TUI остаются not-run. Остановка по Ctrl+C поддержана
общим atomic stop, но настоящий клавиатурный сигнал здесь отдельно не проверялся.
