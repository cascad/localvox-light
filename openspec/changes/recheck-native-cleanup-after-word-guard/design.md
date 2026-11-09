# Design

## Context

См. proposal.md. Свежий worker содержит RV-018/019/020 guards. Доступны неизменённые
72-case/24-boundary corpus, публичная STT 704 строк и предыдущий targeted review.
Text profiler сейчас всегда создаёт Trace, даже без --trace-ollama; это скрытый
обход URL detection, который для проверки качества не требуется.

## Goals / Non-Goals

Оценить новые native cleanup результаты после словарного guard, их полезность,
время и известные риски. Не менять admission/prompt/corpus/default model и не
включать очистители в автоматический маршрут. Не принимать модель для Mac или
всего cleanup по короткому набору/скорости RTX. Не устранять новые риски костылями.

## Decisions

- В обычном text-only по умолчанию Trace=None, endpoint не переписывается, report
  llm_trace=null. Явный --trace-ollama и старые explicit sparse adapters сохраняют
  прежний opt-in путь. Это не рефакторинг product transport: обычный worker напрямую
  использует настроенную Ollama. Missing counters не заменяются нулём.
- Regression инструмента: реальный worker + локальный model-free OpenAI-compatible
  fixture подтверждают прямой URI, все четыре text modes, receipts/privacy и null
  native counters. Старый telemetry fixture запускается с явным флагом; ожидания
  guards не меняются. Проверить explicit compatibility path и existing failures.
- Frozen worker SHA из свежей сборки a744f9c; новый release profiler. Corpus/source,
  evaluator, guards/prompts и .env fingerprints фиксируются до серии и сверяются
  после. New output каждый раз, TEMP/TMP/data на D:, без STT/download/user archive.
- Gemma4:e4b и qwen3.5:4b, по три repeats обоих synthetic corpora (12 worker runs).
  Строгий результат evaluator: 72 expected и 24/24 useful, 24 boundary expected,
  violation/review=0. Missed отдельно от нарушения; exit 2 не выдавать за passed.
  Корпус и ответы не корректировать после генерации. Gate не гарантирует общий смысл.
- Затем четыре ordinary long cleanup: Gemma→Qwen→Gemma→Qwen, один repeat каждый,
  timeout 900 s. Short cases timeout 180 s. Sampling/context/caps штатные,
  native counters не измеряются; wall не объявлять чистым временем генерации.
- До native зафиксировать по модели union прошлых preselected/supplemental review
  indices и обязательных RV controls. На каждом длинном результате проверить их
  и 8 равномерных новых edits по заранее заданному правилу. Новые lexical/adjacent
  flags проверять отдельно как supplemental; не выдавать их за случайную выборку.
  Source/actual/соседи читаются в target. Повторно одинаковые triples можно сверить
  с прошлым заключением по SHA, явно отмечая reuse. Full-corpus accuracy не заявлять.
- Профили проверяет production artifact verifier + прежний evaluator raw hashes/
  source/version/edits. Native генерация отдельно от replay. При новом риске —
  зафиксировать case/RV и не принимать модель, не менять контракт под дефект.

## Risks / Trade-offs

Генерация недетерминирована → повторы и два длинных запуска на модель, не только
один score. Консервативный guard отвергает полезное → missed/usefulness отдельно.
Без proxy нет native tokens/done_reason → null, не утверждать отсутствие truncation.
Desktop/cache/Ollama shared → fingerprints/residency, без exclusive GPU/SLA.
Audio/owner/Mac/refine/summary quality остаются отдельными; смысловая гарантия не выводится.

## Migration Plan

Обновление инженерного profiler/docs/CI и evidence. Worker и настройки приложения
не меняются, daemon/capture не запускаются. Закоммитить проверенный этап без push.
