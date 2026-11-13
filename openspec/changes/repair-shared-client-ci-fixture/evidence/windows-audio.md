# Windows audio apartment lifetime, 2026-11-13

## Hosted result on 873ffa2

[Interface 43436838950](https://github.com/cascad/localvox-light/actions/runs/43436838950),
job 131924033929: UI build/unit/e2e, Rust clients/ConPTY, coordinator/settings,
journal/HTTP, profiling, confirmed input, cancellation и indexing **passed**.
Shared UI/TUI helper **failed**, status `3221225477` (`0xC0000005`), stderr пуст.
Artifacts 14044793135 сохранены в `target/tmp/interface-evidence-873ffa2/`:
timelines успели пройти общий import, note/ask и reconnect. Media/FLAC/spec шаги
после shared test skipped, не passed. Исправленный browser clock не упал.

[Build 43436839096](https://github.com/cascad/localvox-light/actions/runs/43436839096):
Windows x64, Linux x64/ARM64, macOS ARM64 jobs **passed**. Intel Mac job
131924035392 **failed на Upload artifact**, DNS `ENOTFOUND` у GitHub CreateArtifact.
Его daemon/TUI/desktop build, delivery/registration, package/startup и installed
doctor/setup/reconcile passed. Это не полностью green workflow: Mac bundle не
загружен. Publish skipped. В код не добавлен retry для единичного сетевого сбоя.

## Root and native reproduction

В CPAL 0.17.3 `wasapi/device.rs::get_enumerator` сохраняет IMMDeviceEnumerator
в process-wide OnceLock. Его первый caller создаёт thread-local STA, очищаемый
при завершении потока. Следующий caller использует прежний указатель. В HTTP
settings это возможно между короткоживущими request threads.

Это соответствует [upstream CPAL #1302](https://github.com/RustAudio/cpal/issues/1302).
Source release [0.18.2 COM](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/com.rs)
сохраняет STA initialization; простое обновление dependency не устраняет проблему.

Отдельная fresh-process программа с CPAL 0.17.3 перечислила input devices на
первом std thread, join завершился; на следующем thread процесс получил
`0xC0000005`. Ни stream, ни модель, ни пользовательский .env не открывались.
`target/tmp/cpal-apartment-repro.log`.

Контроль с сохранённым MTA support и MTA initialization каждого worker прошёл
8 последовательных закончившихся потоков: `cpal-apartment-owned-mta.log`.
Используется штатный [CoIncrementMTAUsage](https://learn.microsoft.com/windows/win32/api/combaseapi/nf-combaseapi-coincrementmtausage),
сохраняющий MTA, когда explicit worker threads уже завершились.

Repo regression `audio::host_tests::device_discovery_survives_short_lived_threads`
запускает fresh owned helper для каждого сценария. До fix `host-only` получил
тот же native status `3221225477` (audio-host-native-red.log).
После fix **passed**, 0.51 s (audio-host-green.log): host-only, последовательные
и параллельные discovery threads, STA-first отказ перед входом в CPAL.
Список может быть пустым: тест не требует микрофона и ничего не записывает.
Первый предварительный probe с полными DeviceChoice вместо голого iterator не
воспроизвёл native abort локально; его STA-first assertion failed. Сохраняется
как отдельный результат audio-host-red.log, не объявляется тем же native red.

## Implementation

`audio/windows_host.rs` отделяет process MTA support от thread apartment guards.
Инициализация идемпотентна и кеширует только успех; уже существующий STA приводит
к ошибке до обращения к CPAL. Cookie — cross-thread usage token (официально
разрешён Microsoft), не указатель COM interface. Единственный process-scoped
guard живёт столько же, сколько static enumerator CPAL; не освобождается между
запросами, ресурсы процесса забирает ОС при его выходе. Каждый успешный
CoInitializeEx балансируется CoUninitialize в !Send TLS guard.

Все Windows CPAL entrances и WASAPI discovery/watcher/loopback идут через этот
слой. Прежние повторные unbalanced initialize_mta в watcher/discovery удалены.
CPAL 0.17.3, устройство/ID/name и схема settings сохранены; native bug fix,
не новая пользовательская возможность. Нет test-only warmup, подмены списка,
искусственной сериализации HTTP, сна, повтора теста или обновления snapshots.

CFG-001/002/003, ARCH-001, API-003 затронуты доступностью соответствующих API;
новых нормативных требований нет. Runtime/device hardware acceptance не подменяется
тестом enumeration. Сценарии user-guide не изменились.

## Verification

Rust 1.99.1, native x64 Windows, jobs=2, INSTA_UPDATE=no. Rustup toolchain, build,
TEMP и скачанные evidence на D:. Использован существующий Cargo registry;
существующие runtime DLL paths добавлены только в тестовую среду.
Check core passed; Cargo.lock добавил только прямую связь core → windows 0.62.2,
новых версий зависимостей нет.

Полная native debug suite **passed** (audio-host-suite.log): core 441 / 9 ignored,
API 91 / 8 ignored + 3 integration, client 3 + 10 transport, process 12 и resident
I/O harness, TUI 212 / 3 ignored, 8 ConPTY, 4 launcher, 2 I/O.
Ignored helper исполняется четырьмя owned child сценариями родительского теста;
остальные ignored integration/device/model сценарии этим запуском не проверены.

Настоящий shared_workspace **passed** (13.91 s, audio-host-shared.log): embedded UI,
Chromium, реальный HTTP/archive и ConPTY, настройки/notes/ask/reconnect/client quit.
Отдельно real mpv/null-audio acceptance passed (3.72 s, audio-host-media.log).
После очередного cache feature-set build все команды использовали locked graph.

Windows release daemon/worker/TUI/vox build passed (1m10s), desktop passed
(36.82 s). `package-release.py --desktop` собрал и проверил полный ZIP в
`target/audio-host-delivery/`; настоящий installed bundle smoke passed:
check-before-install, expected missing ASR exit 2, doctor JSON, setup cancel/no-op,
reconcile unchanged и read-only fetch check. Модели намеренно отсутствуют;
глобальный PATH/пользовательская установка не менялись.
Логи audio-host-{product-build,desktop-build-fixed,package,install}.log.
Первая desktop команда содержала неправильное имя package и завершилась до
компиляции; использовано штатное имя localvox-desktop, исправленный запуск passed.

Final checks passed: scoped rustfmt core audio/engine/modules, API cargo fmt,
strict main specs, strict обоих затронутых changes, check-openspec, actionlint
обоих workflows, diff. Никаких изменений PNG baseline.
Новый hosted/native Mac/Linux run после этого fix: not-run до push владельца.

## Hosted follow-up on 44df41e

[Interface 43447219612](https://github.com/cascad/localvox-light/actions/runs/43447219612)
прошёл native regression и shared_workspace, а также все UI/Rust/worker/media/FLAC
шаги. Native fix подтверждён hosted выполнением. Последний Specification checks
упал на давнем delta drift, исправление документации описано
[отдельно](../../fix-text-correction-retention/evidence/delta-reconciliation-2026-11-13.md).
Workflow в целом пока failed, remote acceptance 2.3 остаётся открытой.
