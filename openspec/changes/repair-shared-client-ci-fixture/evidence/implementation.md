# Interface CI, 2026-11-13

## Hosted outcome before this repair

Владелец отправил `4b90802137b55e748c7ce1b6081707329d02deab` в main.

- [Build binaries 43395635060](https://github.com/cascad/localvox-light/actions/runs/43395635060):
  **success**, Windows x64, Linux x64/ARM64, macOS Intel/ARM64. Все пять jobs
  собрали и проверили bundles, установку/doctor/reconcile и registration fixtures;
  Windows/macOS также desktop. Release publish пропущен для обычного push,
  bundles загружены в workflow artifacts. Это native hosted execution, не
  аппаратная приёмка записи/микрофона на Mac.
- [Interface 43395635063](https://github.com/cascad/localvox-light/actions/runs/43395635063):
  **failure**, job 131785738780, шаг `Both clients against one actual temporary API/archive`.
  Предшествующие UI unit/browser/visual, TUI/ConPTY, shared coordinator/settings,
  journal/HTTP, profiling fixtures, incremental/cancellation/indexing шаги passed.
  Следующие media/FLAC/spec шаги skipped, не passed.

Артефакт `interface-test-evidence-windows` id 14023438128 скачан в
`target/tmp/interface-evidence-43395635063/`. Terminal/browser timelines показывают
общие артефакты, заметку, ask и reconnect; helper оборвался без panic-отчёта.
Внешний тест скрывал status/stdout/stderr за missing `passed` sentinel.
Конкретная исходная причина до воспроизведения не установлена.

## Local verification

Rust 1.99.1, native Windows x64, targets/tools/temp на D:, jobs=2,
INSTA_UPDATE=no. Native DLL PATH содержит только существующие Vosk/profile dirs.
Тесты используют собственный временный API/archive; модели и capture не запускаются,
пользовательские .env/archive/демон не меняются. Chromium уже установлен.

- Release shared_workspace passed: настоящий HTTP, embedded UI, Chromium, ConPTY,
  общий import/artifacts, summary, audio, notes, ask, settings, auth/reconnect,
  client-only exit. `target/tmp/cross-client-reproduce.log`, 10.28 s.
  Это не доказательство debug/hosted pass.
- API unit: 91 passed / 8 ignored, 8.64 s.
- Оставшиеся interface media проверки: 9 passed — реальный mpv/null audio,
  codec 2, planner seam 1, incremental FLAC seam 1, HTTP media 1,
  sample-accurate range 1, partial/reload 2. Generated PCM, реальный ffmpeg,
  без устройств/моделей. Логи `target/tmp/cross-client-check-*.log`.
- Первая локальная попытка с неверным PLAYWRIGHT_BROWSERS_PATH завершилась 101;
  после использования существующего Chromium release сценарий passed. Эта ошибка
  локальной настройки не объявляется причиной hosted сбоя.

## Diagnostic checkpoint

Исходный hosted сбой **локально не воспроизведён**: прежний ManagedProcess
запуск в debug тоже passed (11.66 s, cross-client-debug-baseline.log).
Замена supervisor не объявляется исправлением исходной причины.

Конечный helper теперь использует существующий bounded process::run с прежним
60 s timeout, normal priority и owned process tree; сохраняются status и
раздельные stdout/stderr. Успех требует exit 0 и прежнего sentinel. Panic hook
пишет файл и вызывает исходный hook; браузер сохраняет ошибку даже до создания
page, а cleanup закрывает browser при сбое trace.stop. Все исходные assertions
и реальные клиенты сохранены.

- Финальный debug shared_workspace passed, затем ещё три последовательных
  прогона passed: cross-client-debug-final.log и cross-client-debug-repeat-1..3.log.
- Новый actual-api-diagnostics unit запускает настоящий browser runner с
  заведомо отсутствующим Chromium: exit 1 и диагностический файл обязательны.
  `npm --prefix ui test`: 15 passed, 0 skipped, 0 failed.
- Ранний debug прогон также passed (6.73 s, cross-client-debug-reproduce.log).

Это завершённый этап диагностики; задачи воспроизведения hosted причины и её
исправления остаются открытыми. Изменения пользовательского поведения отсутствуют,
user-guide не меняется. Финальные проверки passed: cargo fmt API, node --check,
actionlint обоих workflows, strict main specs и этого change, check-openspec
(15 capabilities / 88 requirements / 188 scenarios / 88 coverage rows), diff.
Новый hosted run после изменения: not-run, нужен push владельца.

## Hosted follow-up on d30727b

Владелец отправил diagnostic checkpoint. [Новый interface run 43429701720](https://github.com/cascad/localvox-light/actions/runs/43429701720)
завершился **failure раньше shared-client шага**: UI unit 15 passed, browser
80/81 passed, timing scenario не нашёл точные 5 с при идущем clock.
Shared helper/ConPTY/API шаг в этом run skipped, новой диагностики его завершения
нет. Исходные задачи 1.1/1.2 остаются открытыми. Отдельный подтверждённый flaky
clock исправляется в [stabilize-pipeline-clock-test](../../stabilize-pipeline-clock-test/evidence/implementation.md).
[Build 43429701723](https://github.com/cascad/localvox-light/actions/runs/43429701723)
снова passed на всех пяти платформах; publish skipped для обычного push.
