# Design

## Context

См. proposal.md. Существующие localvox-profile/queue-profile уже запускают production
coordinator с sibling worker, explicit marker/lock и отдельным архивом без capture/API.
Сохранены публичные MP3/PCM Podlodka #109 и fingerprints старого BL-003. Текущая
конфигурация указывает Ollama/qwen3.5:9b; сервис отвечает. Перед сборкой процессов
LocalVox не обнаружено. Последние изменения проверялись отдельными компонентами.

## Goals / Non-Goals

Проверить согласованную свежую сборку и полный маршрут готового аудио через STT
к optional summary, принятие receipts/переходы/длительности. Не менять поведение,
provider/model/prompts/thresholds, настройки и пользовательский архив. Не объявлять
подтверждёнными качество содержания, аппаратный звук, Mac или визуальную приёмку
по одной успешной сборке. Download/conversion исключены: PCM уже подготовлен.

## Decisions

- UI build до native build; daemon/worker/TUI/desktop и оба существующих profiler
  собираются одним locked release набором, jobs=2. TEMP/TMP и outputs на D:.
  Не создавать новый target со второй полной копией dependency cache. Запуск
  рабочего приложения не требуется; hashes новой сборки фиксируются после тестов.
- Программная regression: UI unit/browser e2e с одним worker, client/terminal/
  process tests, core NER/LLM, coordinator и profiler contracts. Пять отказных
  сценариев реального model-free queue fixture (empty/timeout/failed/cancelled/
  incompatible) выполняются с новым release worker через Cargo runtime environment.
- Один свежий сеанс STT-only и один STT+summary на той же записи 7146.527375 s,
  последовательно. Новый output на каждый; существующий headless coordinator,
  настоящие GigaAM/GLiNER и Ollama qwen3.5:9b. Refine/cleanup/diarization off,
  как у прежнего reference. Подготовка копии PCM и проверенный prepare receipt
  отдельно от phase/batch wall; это не замер скачивания/конвертации.
- Текущая .env читается только для выбора несекретных model/endpoint; worker
  environment формируется существующим allowlist profiler. Никаких .env writes,
  скрытого cloud fallback, unload чужой модели или запуска устройств.
- Использовать прямое соединение с Ollama, без существующего --trace-ollama:
  его proxy URL `/:11434/v1` обходит detect_flavor через содержимое пути. Такой
  обход для этой проверки не нужен; исправление typed provider/telemetry относится
  к отдельной работе. Нативные HTTP/token counters остаются null, не подменяются
  оценкой. Trace этого прогона — штатные operation/queue clocks без текста.
  Реальные тексты/аудио остаются в target; в evidence только numeric reports/hashes.
  Receipts проверяет production verify_phase,
  canonical transcript hash не заменять SHA сырых байтов JSONL.
- Timeout по 900 s на маршрут плюс внешний supervisor existing process runner;
  checkpoint остаётся incomplete при отказе. Проверить complete/all_success,
  events_lost, enabled/disabled stages, ready vs unverified, исходные SHA и
  неизменность executables/model/config. Без сборок/других моделей во время замеров.
- Scope ресурсов existing owned-process tree не включает автоматически внешнюю
  Ollama; наблюдение её residency фиксируется отдельно. Нет общего SLA и утверждения
  об эксклюзивной GPU. Старый benchmark не переписывать под новые результаты.

## Risks / Trade-offs

Длительный LLM ответ → bounded timeout/partial, не подгонять caps/prompts. Ошибка
сборки/теста → выяснить причину, не обновлять snapshots автоматически. Нагрузка
desktop/кеш влияют на время → фиксировать условия и разделять вложенные интервалы.
При дефекте контракта исправлять реализацию отдельным связанным change; хрупкие
исключения сначала обсуждать по AGENTS.md.

## Migration Plan

Новая сборка остаётся в target/release для последующей ручной приёмки. Не запускать
daemon/capture/desktop на пользовательском архиве. После regression/native evidence
обновить coverage/catalog и передать точный состав/хеши сборки; commit без push.
