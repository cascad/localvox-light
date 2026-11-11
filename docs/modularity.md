# Модульные границы LocalVox

Рабочая контрольная точка: `b3766b6`. Рефакторинг сохраняет действия, артефакты,
HTTP/CLI, модели/параметры и визуал. План и исполнение —
[OpenSpec change](../openspec/changes/refactor-modular-feature-boundaries/tasks.md).

## Правила модулей

- Сценарий описывает действие и принимает результат. Конкретный backend, endpoint,
  библиотека модели и platform input выбираются в composition/config layer.
- Контракт содержит типизированные данные и ошибки; адаптер владеет SDK/HTTP/процессом.
  Deadline, cancellation и ресурсное ownership нельзя терять при замене реализации.
- ASR принимает mono PCM 16 kHz и возвращает текст; identity/dependencies/recipe
  проверяются до reuse и после load. Неподтверждённый результат не становится Done.
- Захват звука выдаёт PCM/FrameMsg и подтверждённые sealed ranges. UI input выдаёт
  terminal Event → Action. Эти два вида ввода имеют отдельные контракты.
- LLM model/options и messages отделены от способа обращения к модели. HTTP,
  Ollama-native и CLI сохраняют собственные wire/timeout/error правила за границей.
- UI/TUI читают API DTO и workflow. Preferences, data responses, navigation и render
  разделяются; reconnect переносит клиентские preferences целиком.
- Сохранённые форматы читаются совместимо. Модули не узнают участников между сессиями.

## Порты и владельцы

| Граница | Данные и владелец | Точки реализации |
|---|---|---|
| Audio input | PCM/FrameMsg, stop/join и capture lease | core/audio, engine, pipeline → chunks/capture |
| ASR | AsrEngine; model dependencies/identity; один owned runtime | core/asr/package; cook/model policy и cook/runtime |
| LLM | ChatMessage + GenerationOptions → raw content/error; client guards | llm/backend/{http,claude}; LlmClient |
| Worker execution | finite Job/Request → Completed/Cancelled/Failed; supervisor владеет деревом | autocook/execution и warm; process; worker_server |
| Import | настройки tools и PCM recorder общие; manual/queued policies отдельны | ingest/MediaTools; asr/prepare и prepare/manual |
| Artifact acceptance | checksums/receipts → confirmed transition | core/artifacts, processing, jobs, workflow |
| ASR result storage | bounded prepared outputs → durable Accepted → checkpoint/publication | core/incremental/{batch,cache,wal}; cook и cook/prefix |
| Application API | session/document/query/command DTO | API archive feature operations и client |
| Terminal input | Event → Action; поля имеют приоритет | terminal runtime/input и app input |
| Client presentation | preferences + accepted data → view | terminal App/render; React document views/controller |

Существующие порты сохраняются. Конкретные точки замены приведены в таблице;
добавление backend не требует изменения UI/TUI, очереди или правил приёмки.

## Сохранённая совместимость

- Cook определяет язык/recipe и проверяет свежие SHA до reuse и после load. ASR
  package знает файлы/конструктор backend, но не управляет кешем и очередью.
- HTTP dialect выбирается внутри adapter по прежнему правилу URL; Claude CLI
  сохраняет собственные env/model/deadline и stdin. Это не новые настройки.
- Queue принимает только проверенные артефакты, сохраняет исход и повторяет запись
  при отказе storage без нового worker. Execution не меняет очередь. Его journal
  guard остаётся жив до завершения приёмки, включая ожидание сохранения.
- Text/refine/cleanup и старые disk/wire-поля сохранены для явного CLI и старых
  заданий. Штатная цепочка — обязательный ASR и опциональный summary.
- Ручной и queued import сохраняют свои settings filenames, создание сессии,
  feedback, title/receipt/retry правила. Общие tool resolution/PCM recorder не
  означают выполнения полного критерия BL-006.
- User-guide сохраняет те же US-ID и действия. Контракты/значения defaults/модели/
  рецепты не меняются; эту карту не следует читать как аппаратную приёмку.

## Карта пользовательских сценариев

Таблица указывает владельцев и места проверок. `test-linked` означает найденную
проверку группы, а не выполненную приёмку каждого US. Новые прогоны фиксируются
в evidence change. Подробное требование/доказательство —
[coverage](../openspec/evidence/coverage.md); человеческое описание — [user-guide](user-guide.md).

| US | Сценарий | Владелец | Проверки |
|---|---|---|---|
| US-001 | Записать встречу | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-002 | Закончить встречу | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-003 | Разделить встречи | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-004 | Расшифровать ссылку | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-005 | Обойтись без долгой постобработки | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-006 | Понять, что происходит | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-007 | Прочитать технический журнал | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-008 | Возобновить остановленный этап | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-009 | Дождаться автоматического повтора | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-010 | Пережить временный отказ сохранения | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-047 | Отменить обработку | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-050 | Распознавать готовые куски заранее | capture / jobs / worker / workflow | test-linked: core; ASR incremental_fixture; coordinator prefix_tests; UI e2e; ConPTY |
| US-011 | Открыть результат встречи | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-012 | Забрать весь текст | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-013 | Скопировать другой документ | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-051 | Скопировать реплику | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-053 | Скопировать часть сводки | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-014 | Сверить реплику со звуком | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-015 | Управлять прослушиванием | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-016 | Переключить запись во время прослушивания | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-017 | Проверить сводку по источнику | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-018 | Подтвердить проверенный документ | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-019 | Выбрать рабочую версию | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-020 | Изменить язык записи | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-021 | Подписать участников | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-022 | Пересоздать результат | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-023 | Удалить запись | archive documents/media/mutations; UI/TUI documents/player | test-linked: API archive/media/mutations; client; UI e2e; terminal/ConPTY; аппаратный звук not-run |
| US-024 | Найти сказанное | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-025 | Спросить у архива | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-026 | Спросить по своему тексту или файлу | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-027 | Вернуться к ответу | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-028 | Записать заметку | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-029 | Выбрать место для заметки | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-030 | Удалить заметку | archive search/asks/notes; search/voice/integrations | test-linked: API HTTP/workspace; search/LLM/voice; UI e2e; terminal tests |
| US-031 | Надиктовать мысль | voice / routes / capture producers | test-linked: core/voice/integrations; реальные устройства not-run |
| US-032 | Сохранить недавно прозвучавшее | voice / routes / capture producers | test-linked: core/voice/integrations; реальные устройства not-run |
| US-033 | Управлять голосом встречей и ссылкой | voice / routes / capture producers | test-linked: core/voice/integrations; реальные устройства not-run |
| US-034 | Передать локальное аудио/видео | voice / routes / capture producers | test-linked: core/voice/integrations; реальные устройства not-run |
| US-035 | Работать через внешние инструменты | voice / routes / capture producers | test-linked: core/voice/integrations; реальные устройства not-run |
| US-036 | Изменить параметры | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-037 | Настроить модели, источники и хранение | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-038 | Подключиться с токеном | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-039 | Выбрать оформление | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-040 | Восстановиться после ошибки связи | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-052 | Вернуться на уровень выше | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-054 | Свои клавиши TUI | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-055 | Выгружать ASR из памяти | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-056 | Сделать список шире или уже | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-041 | Закрыть терминальный клиент | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-042 | Открыть окно или файлы из трея | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-043 | Приостановить запись аудио | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-044 | Запускать при входе в Windows | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-045 | Остановить всё приложение | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-046 | Проверить готовность установки | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-048 | Посмотреть ошибки TUI | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |
| US-049 | Выбрать или добавить LLM-сервер | settings/config; client; terminal preferences/runtime/input | test-linked: API settings; presets; UI e2e; terminal state/ConPTY; реальные OS bindings not-run |

## Критерий завершения

Все сценарии имеют владельца и проверку/явный not-run; клиенты, API и inference
меняются в своих границах. Существующие snapshots/HTTP/CLI/window identities и
recovery проходят проверки. Итоговая сборка проверена, этапы закоммичены.
Ранний default, исторический legacy budget и аппаратная приёмка остаются отдельными;
BL-006 не считается завершённым только из-за реорганизации CLI.
