# Принятие исхода и пользовательский справочник · 2026-11-01

PIPE-008/010/011: реализация и программные проверки на временных данных.
Это не полное соответствие capability, аппаратная приёмка или обновление демона.

## Воспроизведение и реализация

Новый core-тест до исправления получил Failed/last_error/новые времена в памяти
при несохранённом отказе и прежнем Running на диске. `target/outcome-red.log`:
0 passed, 1 failed. Последующий успешный прогон не заменяет этот результат.

В [jobs.rs](../../../../crates/localvox-light-core/src/jobs.rs) QueueStorageError
типизирует read/write failure внутри прежнего anyhow API. Finish использует общий
rollback commit_jobs. Try_mark_failed возвращает Result и откатывает снимок;
совместимый mark_failed сохраняет сигнатуру. Проверка артефакта и неверного задания
не превращается в storage error, повреждённая очередь не перезаписывается.

В [autocook.rs](../../../../crates/localvox-light/src/autocook.rs) принятие результата
и отказа повторяется только для QueueStorageError до успеха/stop. Между попытками
mutex освобождён; вызывающая линия сохраняет execution lock. Одна операция
«Ожидание сохранения очереди» объясняет ожидание и закрывается при выходе;
она не создаёт новую строку на каждой паузе и не объявляет годность артефакта.
Проверка артефакта остаётся на каждой попытке принятия; ускорение SHA-256 этим
срезом не заявляется. Failed stage дописывается после принятого отказа.

## Существенные сценарии

- Core: новый тест проверяет оба предела attempts=1/max=1 и max=3; checked и
  совместимый API, полный снимок QueueFile, исходные байты и сохранение причины/
  бюджета после снятия отказа на том же объекте. Дополнены существующие тесты
  corrupt queue, invalid artifact и failed finish для различения типов ошибок.
- Три новых Windows-теста используют настоящий deny-replace handle jobs.json:
  принятие валидного синтетического артефакта после release без Failed, исходная
  ошибка worker после release с единственным Failed event и stop для обоих исходов.
  До release файл прежний; ожидающий helper не завершён. Проверены сохранение
  execution_id, отсутствие дополнительных попыток, start/end одной операции,
  доступность queue mutex и удержание session lease во время ожидания.
- Четвёртый новый coordinator-тест подтверждает обычный отказ невалидного артефакта:
  нет storage wait и принятия следующей фазы, исходный бюджет не сбрасывается.

Это настоящие функции принятия исхода с настоящими временными файлами, не запуск
полного демона с моделями. Отсутствие повторного spawn следует из этих функций
и расположения ожидания внутри run_step под lease; аппаратный сбой не воспроизводился.

Первые два main-прогона неуспешны: `outcome-main-first.log` и
`outcome-main-second.log`. Новый test poll сначала не учитывал краткий Windows
lock журнала (os error 33), затем читал Query по умолчанию с limit=0, который
ограничивается одной последней строкой и скрывает operation_start. В helper
добавлены повтор чтения до прежнего deadline и явный limit=200. Вторичные отказы
старого теста — poisoned POST_ENV_LOCK после падения fixture. Продуктовые лимиты
и retry policy ради этих прогонов не менялись; окончательный полный main прошёл.

## Справочник

По прямому поручению владельца создан [docs/user-guide.md](../../../../docs/user-guide.md):
46 стабильных US-ID, короткие строки «действие → ожидаемый результат», UI/TUI,
голос, CLI, MCP и Windows tray. В [AGENTS.md](../../../../AGENTS.md) закреплено
обновление затронутых строк вместе с поведением. В docs index и исторических
S1–S11 добавлены ссылки; старые сценарии/статусы фаз не переписаны как новые проверки.

Сверка по коду: App/DocumentPane/RecordBar/Queue/Journal/Ask/Notes/Settings/Player,
TUI app/commands/forms/io, voice/lib, localvox-process CLI, localvox-mcp,
main tray menu и README --doctor. Все действия палитры сгруппированы в справочнике:

| Группа действий | Сценарии |
|---|---|
| Import/start/stop/retry, журнал | US-001–010 |
| Versions/language/participants/source, recook/confirm/delete session | US-018–023 |
| Copy/save/play/fragment/seek | US-012–017 |
| Search/chat/note/route/ask/file/delete note/delete ask | US-024–030 |
| Edit/reset/save/filter/discard settings, connection | US-036–038 |
| Theme/ASCII/monochrome/color depth, refresh/restore draft | US-039–040 |

Формы/навигация/выход описаны в начале и US-041; голос и внешние входы — US-031–035,
tray/doctor — US-042–046. Это code-reviewed соответствие описаний текущим входам,
а не ручной runtime-прогон всех 46 сценариев. Например, экспорт отказывает при
существующем файле, UI не имеет кнопки импорта локального аудио, голосовая квитанция
доставки остаётся известным gap, проверка выбранного провайдера нужна перед отправкой.

## Выполненные проверки

TEMP/TMP всех тестовых процессов направлены в
`D:\rust_repo\localvox-light\target\test-temp`; глобальное окружение не менялось.

| Команда | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-core --lib jobs:: -- --test-threads=2` | PASS: 60/60 |
| Та же команда с `workflow::`, `progress::`, `journal::` | PASS: 11; 10 + 1 ignored; 16 соответственно |
| `cargo test --locked -j 2 -p localvox-light --bin localvox-light -- --test-threads=2` | PASS: 12 passed, 1 ignored, 3.27 s; четыре новых coordinator-теста |
| `cargo test --locked -j 2 -p localvox-light-api -- --test-threads=2` | PASS: 64 lib + 3 integration; 6 ignored |
| `npm --prefix ui run build`, `npm --prefix ui test`, `npm --prefix ui run lint` | PASS: bundle, 9/9 unit, прежние 8 lint warnings |
| `npm --prefix ui run test:e2e -- --workers=2` | PASS: 68/68, 57.4 s; три намеренных визуальных повреждения дают ожидаемый отказ, PNG не обновлялись |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` после полного browser | PASS: 1/1, 5.37 s, 69 отфильтрованы; настоящий временный API/UI/TUI |
| `openspec validate --specs --strict --no-interactive`, CLI 1.13.2 | PASS: 15/15; прежние INFO о длине требований |
| `openspec validate persist-worker-outcomes --type change --strict --no-interactive` | PASS |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 main coverage rows |
| `rustfmt --edition 2021 --config skip_children=true --check` для jobs.rs/autocook.rs, `git diff --check` | PASS |
| Проверка справочника и новых текстовых файлов | PASS: 46 уникальных US-ID, локальные ссылки, конечные переводы строк и отсутствие trailing whitespace |

Логи текущих проверок: `target/outcome-{jobs,workflow,progress,journal,main,api,shared}.log`
и `target/outcome-ui-{build,test,lint,e2e}.log`. Существующие CI jobs/main покрывают новые tests;
добавлена строгая валидация change. Удалённый CI не запускался.

## Границы

Не менялись модели, схемы данных, лимиты попыток/автоповторов, действия клиентов.
Это не аудит mark_done/clear_force/delete/orphan cleanup, не общий монитор I/O
и не транзакция queue/progress. Другие ignored media/historical/profiling tests
не объявляются выполненными. BL-002/003/004, RV-007/009 и физическая приёмка 10.2
остаются открытыми. Пользовательский архив и работающий демон не трогались.
Main specs не синхронизированы, change не архивирован, коммиты делает пользователь.
