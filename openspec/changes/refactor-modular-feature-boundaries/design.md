# Design

## Context

См. proposal. Рабочая точка b3766b6. Крупные внешние границы уже есть:
capture/API daemon, owned worker, HTTP clients, ASR trait, LLM client, input reader.
Внутри App смешаны приём данных/ввод/навигация; reconnect копирует preferences
поштучно. DocumentPane содержит несколько самостоятельных views, Archive —
поиск/вопросы/документы/запись/мутации. Cook знает конкретный ONNX adapter.

## Goals / Non-Goals

**Goals:** локальная замена реализации через типизированный вход/результат,
сохранённый контроль ошибок/времени/владения и ясный composition root.

**Non-Goals:** новые модели/транспорты/фичи, смена рецептов, storage migration,
новый plugin framework или изменение публичных команд/схем/значений по умолчанию.

## Decisions

- Граница определяется ответственностью и меняющейся реализацией. Использовать
  существующие AsrEngine, ChatMessage, PCM/sealed ranges, HTTP DTO, terminal Event.
  Trait нужен для реальной замены/инъекции, а не для каждого вспомогательного метода.
- ASR package/backend construction и список model dependencies принадлежат ASR
  adapter layer. Cook выбирает язык/recipe и проверяет identity; scheduler не знает
  ONNX/Vosk. Полная fresh SHA и проверка после load сохраняются.
- LLM отделяет model/options от HTTP/CLI transport. Сценарии summary/cleanup/chat
  вызывают единый клиент; конкретный transport выбирается в construction/config.
  Ошибки, лимиты, payload/response validation, keep-alive и model options сохраняются.
- TUI preferences — один объект; reconnect переносит его целиком. Input events,
  data responses, actions и renderer имеют собственные модули. Platform input
  нормализуется до прежнего Event. Захват аудио уже выдаёт независимые PCM/FrameMsg;
  аппаратные реализации остаются за этой границей, ownership не меняется.
- React views разделяются по документам; общий controller владеет polling/control/
  copy. Archive остаётся совместимым фасадом, операции группируются по фичам.
  API DTO и workflow/receipts остаются общей границей для UI/TUI.
- Coordinator сохраняет queue/lease/claim/acceptance; процессные детали выделяются
  в исполнение. Общие операции импорта и config builders имеют одно место, но
  deliberate legacy policies/гарантии не подменяются новыми при рефакторинге.
- Старые disk/wire поля читаются через прежние compatibility paths. Удаление кода
  требует доказательства отсутствия callers/контрактов. Не превращать исторические
  proposal-only или hardware пункты в выполненные переносом файлов.

## Risks / Trade-offs

- Потеря preference/epoch или stale partial → существующие state/ConPTY/reconnect и
  явные tests изолированного App reset; snapshots остаются прежними.
- Выбор backend меняет cache identity/options → fixtures, model dependency/load
  checks, HTTP/CLI wire tests и native ASR smoke на публичном PCM.
- Новые модули скрывают прежние side effects → façade сохраняет методы, cancellation/
  commit boundaries и тесты; методы внутри принадлежат фиче, low-level I/O адаптеру.
- UI extraction меняет DOM/copy/layout → unit/browser e2e и unchanged screenshots.
- Cleanup ломает старую очередь → saved-state/crash/retry/force/legacy tests.

## Migration Plan

Сначала карта фич/границ, затем отдельные проверенные срезы TUI, UI/API, worker и
inference/config. Каждый срез сохраняет рабочую сборку; финальная регрессия и
согласованный bundle завершают этап. Пользовательский архив/демон не запускать.
Реальные устройства и Mac остаются not-run, default раннего режима off.
