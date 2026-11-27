# Capture feedback и macOS metadata · 2026-11-26

## Вход и подтверждённые дефекты

Владелец сообщил, что на Mac микрофон работает, системный звук отсутствует
даже при явно включённом loopback. Наличие разрешения системного аудио ему
неизвестно. Точные CoreAudio errors/список устройств этого Mac не получены.
Предыдущие PASS поведенческого набора не включали реальный захват устройств.

Подтверждено ревью: daemon build не встраивал Info.plist с описаниями доступа;
ошибка loopback попадала в stderr и необязательный UiMsg, а daemon запускает
engine с None. CPAL 0.17.3 открывает duplex output как input; callback error
только логировался, owning loop оставался жив без восстановления.

Это подтверждённые дефекты исходников. Причина тишины на конкретной машине
пока не доказана; восстановление реального звука на Mac не заявляется.

## Реализация REC-004

- [Build metadata](../../../../crates/localvox-light/build.rs) встраивает
  [daemon Info.plist](../../../../assets/macos/daemon-Info.plist) в Mach-O;
  версия подставляется Cargo. [Tauri Info.plist](../../../../src-tauri/Info.plist)
  содержит те же usage descriptions для desktop bundle. Разрешения ОС
  не выдаются автоматически: требуется подтверждение пользователя.
- [Monitor / callback bridge](../../../../crates/localvox-light-core/src/audio/status.rs)
  различает starting/waiting/signal/error/disabled. Callback передаёт evidence
  через atomic flag/неблокирующую очередь; capture loops меняют только память.
  Существующий device-health worker публикует изменившийся snapshot, без новой
  фоновой polling-петли и без fsync в WASAPI packet reader.
- [Capture](../../../../crates/localvox-light-core/src/audio.rs) передаёт callback
  failure в обычный supervisor/retry путь; duplex Mac output не подменяет sys
  микрофоном, возвращает понятное ограничение. Отдельный duplex tap не реализован.
- [API](../../../../crates/localvox-light-api/src/archive/recording.rs) читает
  capture_sources только при живом engine; новый engine инициализирует snapshot.
  Сигнал означает, что он поступал после открытия, а не подтверждение WAV на диске
  или текущего уровня. Тишина не классифицируется как PermissionDenied.
- [TUI](../../../../crates/localvox-light-terminal/src/app/responses.rs) показывает
  значки, details в настройке источника и capture errors в своей консоли.
  [GUI](../../../../ui/src/CaptureSources.tsx) показывает компактную свёртку
  с текущими состояниями и ограниченной историей ошибок. Опрос и повторное
  открытие без сигнала не создают новые unread для того же отказа; signal/disabled
  или смена engine завершают серию. Ошибки не приписываются фазе транскрибации.
- [Native metadata checker](../../../../scripts/check-macos-capture.py) читает
  настоящую __TEXT,__info_plist секцию. Native Mac build gates и behavior runner
  проверяют готовый daemon, а не только XML в репозитории. Recording group с
  --installed-dir проверяет именно установленный daemon, source build его не заменяет.
- Справочник US-001/037 и инструкция проверки обновлены в той же работе.

## Выполнено локально — Windows

Один Cargo job/test thread, собственные fixtures; пользовательские archive,
daemon, установленный bundle, модели и настройки не изменялись.

| Проверка | Результат | Локальный лог |
|---|---|---|
| Audio / callback bridge / lifecycle | 11 passed, 2 ignored | target/review/macos-capture-core-final.log |
| API library suite | 102 passed, 8 ignored | target/review/macos-capture-api-full.log |
| Настоящий HTTP состояния и stale engine snapshot после отделения publisher | 1 passed | target/review/macos-capture-api-final.log |
| Полный TUI state/render suite | 262 passed, 3 ignored | target/review/macos-capture-tui-final-full.log |
| Настоящие browser + TUI + private API/archive | 1 passed, 6.15 s | target/review/macos-capture-shared.log |
| GUI unit / build | 20 passed / build passed | target/review/macos-capture-ui-unit.log, macos-capture-ui-build.log |
| Browser: sources/retry/ack/recovery/disabled/restart, 480 px Stop | 1 passed | target/review/macos-capture-browser.log |
| Тот же сценарий в React StrictMode | 1 passed | target/review/macos-capture-strictmode.log |
| Runner, metadata parser и CI contracts | 32 tests, OK, 1 skipped | target/review/macos-capture-python.log |
| Windows daemon/installer/vox/TUI binaries | build passed | target/review/macos-capture-binaries-final.log |

Mach-O parser проверен synthetic binary с реальной структурой load commands:
valid section, отсутствие секции/обоих обязательных ключей. Это не native Mac
link/run. Единственный Python skip требует явно указанного полного native archive.
Скриншот 480 px осмотрен: Stop виден, popup помещается, подробности не сдвигают
страницу, старые ошибки подписаны историей. Golden screenshots не изменялись.

Локальные логи относятся к последовательным стадиям этой работы: полный API
suite выполнен до выделения callback bridge/publisher, отдельный HTTP regression
повторён после изменения publisher. Это не единый full-suite native hardware run.

Проверка форматирования затронутых Rust файлов, strict validation change и всех
main specs, `scripts/check-openspec.py`, `git diff --check` — passed. Checker:
15 capabilities, 88 requirements, 188 scenarios, 88 coverage rows. Нормативные
main specs ещё не sync, change остаётся открытым до аппаратной задачи 3.2.

## Оставшаяся приёмка

Native macOS сборка с секцией metadata, первый системный consent, ранее
отклонённый доступ, реальный звук другого приложения и независимость от
микрофона — **not-run** в этой Windows-сессии. Новый Linux capture run также
не выполнялся. Наличие description и успешно открытого потока не доказывает
выданное TCC разрешение. Задача 3.2 остаётся открытой.

## Основания решения

- [Apple: CoreAudio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps) — usage description и системный consent.
- [CPAL: известная тишина при отсутствии AudioCapture access](https://github.com/RustAudio/cpal/pull/1257) — предложенные helpers ещё не приняты; частный TCC API не добавлен.
- [Tauri: расширение Info.plist](https://v2.tauri.app/distribute/macos-application-bundle/) — metadata desktop bundle.

## Живой уровень и исправление CoreAudio, 2026-11-26

В CPAL 0.17.3 подтверждён отключённый tap autostart. В поставляемом через
Cargo.lock CPAL 0.18.2 включён autostart и проверяется результат создания tap.
Это известная причина тишины. Версия ОС, разрешения и маршрут конкретного Mac
пользователя в этой проверке не измерялись.

Проверено на Windows в изолированных fixtures:

- Core audio: 12 passed, 2 ignored. PCM через callback → два независимых чтения
  уровня, сброс при переоткрытии, известная тишина, срок годности и скачок часов.
- Весь API: 102 passed, 8 ignored. Включая реальный HTTP для уровней, имени
  устройства, остановленного движка и обратной совместимости.
- Весь TUI: 262 passed, 3 ignored. Отдельные шкалы, срок годности, ошибка
  источника и его имя в настройках.
- GUI build, 20 unit. Два сценария источников в Chromium дважды на production
  build и один раз под StrictMode. Проверены независимые уровни до записи,
  пауза, зависший запрос, восстановление, Stop на экране 480 px.
- Windows daemon и TUI собраны с `--locked`. Strict change и 15 main specs,
  checker и diff проверены.

Проверка выхода в наушниках и настоящего системного диалога macOS остаётся
в задаче 3.2. Аппаратный звук Windows и Linux также не измерялся.


## Уведомление аудиобуфера после Stop · 2026-11-27

CoreAudio сообщает о перегрузке устройства через [CPAL Xrun](https://docs.rs/cpal/0.18.2/cpal/enum.ErrorKind.html#variant.Xrun). Раньше тот же обработчик закрывал поток при любой ошибке. Теперь регрессионный тест проверяет перегрузку до первых семплов, поступление PCM на том же потоке, независимость второго источника, накопление счётчика и последующий DeviceNotAvailable. На Windows прошли 13 тестов `localvox-light-core --lib audio::` (два помечены ignored) и тест чтения состояния источников через настоящий HTTP. Аппаратный сценарий после остановки на Mac остаётся открытым.
