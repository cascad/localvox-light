# Очередь во время обнаружения работ · 2026-10-31

Продолжение после `45a1808`; принятые PIPE-001/002/009/011/013 и delta PIPE-002/016.
Обход архива в coordinate выполнялся внутри JobQueue::mutate: чтение и проверка
каждого транскрипта удерживали общую блокировку, необходимую worker для claim и
API для сохранения импортированного задания. Чтение готового jobs.json само по
себе не блокировалось; проблема касалась переходов/изменений очереди.

## Воспроизведение и реализация

Две регрессии [координатора](../../../../crates/localvox-light/src/autocook/discovery_tests.rs)
останавливают сканирование на управляемом барьере. Пока барьер закрыт, отдельный
поток пытается взять Pending, а второй сценарий отправляет настоящий HTTP POST
типизированным клиентом через штатный router. До выноса scan из критической секции
оба теста упали: действие завершалось только после открытия барьера. После
исправления оба завершаются при всё ещё остановленном сканировании. Двухсекундное
ожидание в тесте — страховка от зависания, не численное SLA приложения.

[discovery](../../../../crates/localvox-light-core/src/jobs/discovery.rs) читает
очередь и архив вне mutex. До проверок сохраняются снимок Job, SHA-256 содержимого
meta.json/versions.json/processing.json, создание каталога и список аудиофайлов
(имя, размер, mtime). Перед публикацией одного кандидата заново загружается очередь,
сравниваются Job и метаданные, проверяется recording marker и берётся неблокирующий
lease сессии. Это временная защита от устаревшего решения, не постоянный кеш
проверок артефактов. Полные проверки transcript/receipt сохраняются.

Каждый кандидат публикуется в своей критической секции. Изменившееся или занятое
откладывается до нового обхода. Текущая настройка summary читается при публикации;
выключение summary не выключает обязательный STT. Дополнительно
[JobQueue::start](../../../../crates/localvox-light-core/src/jobs.rs) повторно
проверяет активную запись: если запись началась после постановки, Pending остаётся
ожидать без расходования попытки.

[Семь core-сценариев](../../../../crates/localvox-light-core/src/jobs/discovery/tests.rs)
проверяют однократную постановку, изменение политики, ручной запрос, завершившуюся
новую попытку, удаление, активную запись до/после публикации, занятый lease,
изменения metadata/версии/receipt/аудио, битую очередь, обязательный STT. Отдельно
проверена замена meta.json тем же числом байт с восстановленным mtime: старый
кандидат всё равно отвергается. Сканирование не перезаписывает повреждённый jobs.json
и не воссоздаёт удалённый каталог.

При проверке удаления найден дополнительный риск: VersionStore::open создавал
transcripts даже при чтении. Для обнаружения добавлен внутренний
[VersionStore::for_read](../../../../crates/localvox-light-core/src/versions.rs)
без инициализации каталогов; тест проверяет отсутствующую, пустую и удалённую
сессию. Это исключает создание каталога старой сессии самим чтением во время обхода.

## Измерение задержки claim

Временный архив из 4 либо 16 записей, каждая — 7200 синтетических JSONL-реплик
с таймкодами на 7200 секунд, валидными STT/summary receipts. Аудио и модели не
запускаются. Пока координатор действительно читает этот архив, другой поток
пытается взять отдельное Pending и сохранить Running. Один прогрев, 3 измерения;
min/max и медианы сохранены в [before](queue-discovery-before.json) и
[after](queue-discovery-after.json).

Windows, dev-сборка; имя CPU AMD Ryzen 9 9900X3D, окружению доступны 6 логических
CPU. Baseline — выделенный helper с теми же входными данными и защитой публикации,
но scan ещё находится внутри queue mutex. Измеряется именно эффект освобождения
очереди, не разница между двумя скоростями модели или аппаратная приёмка.

| Записей | Обход before → after, мс | Ожидание и commit claim before → after, мс |
|---:|---:|---:|
| 4 | 200.879 → 202.924 | 203.782 → 2.927 |
| 16 | 805.657 → 834.549 | 817.419 → 3.229 |

Обход продолжает требовать чтения архива, но больше не определяет задержку claim.
Числа относятся к этой синтетической нагрузке с прогретым файловым кешем;
гарантированного предела для произвольного диска/нагрузки здесь не устанавливается.

Команда из корня репозитория:

```powershell
$env:LOCALVOX_DISCOVERY_PROFILE_OUTPUT = Join-Path (Get-Location) 'target/discovery-profile.json'
cargo test --locked -j 2 -p localvox-light --bin localvox-light --no-default-features autocook::discovery_tests::profile_queue_discovery -- --ignored --exact --nocapture --test-threads=1
```

Profiler ignored в обычном запуске. Для сетевой регрессии в dev-dependencies
демона добавлен уже существующий localvox-light-client; новых runtime-зависимостей нет.

## Выполненные проверки

Cargo запускался с `--locked -j 2`; обычные Rust-прогоны с `--test-threads=2`.
Локальные производные логи: `target/discovery-*.log`.

| Проверка | Результат |
|---|---|
| `cargo test -p localvox-light-core --lib --no-default-features` | 335 passed, 1 profiler ignored; включая 7 discovery-сценариев и VersionStore без mkdir |
| `cargo test -p localvox-light --bin localvox-light --no-default-features` | 6 passed, 1 profiler ignored; оба барьерных сценария claim/POST прошли после исходного воспроизведения двух отказов |
| `cargo test -p localvox-light-api --lib` | 59 passed, 6 ignored; регрессии импорта, очереди, HTTP и настроек |
| `cargo test -p localvox-light-client -p localvox-light-terminal -p localvox-light-process` | 111 passed; 3 внутренних helper ignored, отдельный real-media ignored |
| `npm run test:e2e` | 33 passed, включая 3 ожидаемых отрицательных проверки детектора; PNG baseline не менялся |
| Сборка daemon/worker/TUI | PASS, dev-комплект; модели не запускались |
| `cargo test -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | 1 passed, последний прогон 5.79 s; настоящий temp API, Chromium UI и TUI/ConPTY |
| `openspec validate --specs --strict --no-interactive` | 15 passed, 0 failed |
| Строгая валидация control-postprocessing-and-terminal-player | PASS |
| `python scripts/check-openspec.py`, `git diff --check` | PASS: 86 main требований, 168 main сценариев, 86 строк main coverage; delta отдельно |

## Границы

Работающий демон, пользовательская .env, архив и engineering-doctrine.md не
менялись. Полная модельная обработка и физическое оборудование не запускались;
BL-003 и аппаратная приёмка прежнего change остаются открытыми. Несколько независимых
процессов, пишущих jobs.json, по-прежнему не поддерживаются. Лимит попыток, проверка
SHA-256, приоритет аудио и правила старых/ручных заданий сохраняются.
