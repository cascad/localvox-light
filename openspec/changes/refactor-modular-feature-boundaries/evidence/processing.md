# Срез 3 · Inference, execution и import, 2026-11-11

## Границы

- `core/asr/package` владеет выбором ONNX/Vosk, именем, списком зависимостей и
  загрузкой. `cook/model` сохраняет language/recipe, fresh content SHA перед reuse
  и повторную проверку после load. Fixture/native/legacy paths не смешиваются.
- `ChatBackend` принимает messages и `GenerationOptions`; конкретный HTTP/Ollama/
  Claude transport скрыт в `llm/backend`. Общий LlmClient сохраняет stripping
  thinking/trim/empty guard и обновляемый max_tokens. Новых провайдеров не добавлено.
- `autocook/execution` возвращает Completed/Cancelled/Failed, владеет командой,
  resident/one-shot transport, heartbeat и journal operation. Очередь/claim/lease,
  проверка артефактов, acceptance и повторное сохранение исхода остаются в
  coordinator. Guard живёт до окончания acceptance, включая storage wait.
- Импорт находится в `asr/prepare` и `prepare/manual`. Оба используют общий PCM
  recorder и `ingest::MediaTools`. CLI glue больше не содержит запись chunks.

## Совместимость и очистка

Удалена только недостижимая ветка `finish=false`: единственный вызов передавал
literal true; отдельного caller незавершённой Text-приёмки не существовало.
Сохранены Text/refine/cleanup для старых заданий/явного CLI, force, protocols,
disk formats, defaults и рецепты моделей. `.env.defaults` не менялся.

HTTP dialect сохраняет прежнее URL-правило внутри adapter; Claude сохраняет
свою env-конфигурацию на запрос, model/provenance и процессные лимиты. API profile
factory и CLI flags остаются разными composition roots с прежними приоритетами.
Устаревшие active architecture snippets обновлены: штатно ASR + optional summary,
ранний ASR выключен по умолчанию; resident transport отделён от one-shot I/O.

BL-006 остаётся предложением: settings filenames, создание новой/существующей
сессии, titles/receipts/retry и cleanup semantics не уравнены переносом. Проверка
PCM обнаружила существующее различие ротации: queued feed передаёт всю дорожку,
manual feed — блоки. Первое предположение нового теста о двух chunks для обоих
входов было неверным; тест исправлен на проверку обеих исходных политик и точное
побайтовое восстановление всего PCM. Производственный алгоритм не менялся.

User-guide не меняется: US-сценарии и доступные действия сохранены. Исторические
доказательства/performance gates не переписываются; аппаратная/Mac/реальные LLM
проверки этим рефакторингом не заявляются.

## Прогоны

Общий Rust-прогон: **908 passed**, без failures. В том числе core **462**, LLM
**113 + corpus**, ASR **1 PCM + 5 child-worker fixtures**, ingest **5**, API
**81 + 3 integration**, coordinator **15**, process **12**, TUI **198 + 5 ConPTY**,
IO/run-script и fbank golden. Ignored device/helper/explicit сценарии не выданы
за пройденные. Snapshots не обновлялись, `INSTA_UPDATE=no`.

Отдельно через Cargo пройдены **8 real-worker tests**: finite prefix с ростом,
Stop/disable/cancel, timeout/error/missing/changed input/retry и запретом повторного
ASR принятого префикса; отмена summary до API acknowledgement; отказ в отмене
standalone CLI; idle/yield/timeout/unavailable/concurrent indexing. Ещё **1 real
UI/TUI shared HTTP** прошёл с установленным Chromium и настоящим ConPTY, временным
архивом и синтетическими processing artifacts, без моделей/устройств.

Новые loopback HTTP tests проверяют фактические wire/auth/messages/options/response
и error context для OpenAI-compatible и Ollama-native. Fake backend проверяет
сменяемость порта/current max_tokens/common guards и сохранение ошибки. Прежние
Claude invocation/proxy/error/JSON parsing tests сохранены. Native smoke и release
bundle фиксируются в итоговой проверке; новые модели и качество LLM не оценивались.

Логи: ignored `target/tmp/modular-all-rust.log`, `modular-real-workers.log`,
`modular-cross-clients.log`. Прямой запуск тестового EXE вне Cargo не завершил
startup и был прекращён; все заявленные explicit результаты получены штатным
`cargo test`, с runtime окружением Cargo. Этот неудачный старт не является test-run.
