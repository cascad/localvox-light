# Документация localvox — индекс

[OpenSpec](../openspec/README.md) — **принятый контракт поведения**:
[каталог](../openspec/catalog.md) → [пайплайн](../openspec/specs/processing-pipeline/spec.md) →
[покрытие каждого требования](../openspec/evidence/coverage.md) → [расхождения](../openspec/review.md).
[Baseline принят 2026-09-27](../openspec/baseline.md); соответствие реализации и
результаты проверок учитываются отдельно. README и прежние документы производные.

Конкретные работы ведутся в штатных [OpenSpec changes](../openspec/catalog.md#предложения),
доступных через `openspec list`; [порядок работы и skills](../openspec/README.md#штатный-рабочий-процесс).
Прежний [бэклог](../openspec/backlog.md) служит указателем BL-ID и входящим списком сырых идей.

[Архитектура исполнения](architecture.md) описывает процессы и модули;
[диагностика пайплайна](processing-pipeline.md) помогает разобрать конкретную запись.

Другие документы и история решений:

| Документ | Разрез | Что внутри |
|---|---|---|
| [feature-registry.md](feature-registry.md) | **Исходный реестр** | Принципы P1–P9 и группы F1–F11; описания по коду и границы проверки — в OpenSpec |
| [work-plan.md](work-plan.md) | **История пакетов работ** | Stage 0 (замеры) + фазы A–D: пакеты работ, задачи, критерии приёмки, риски |
| [feature-backlog.md](feature-backlog.md) | **Фичи (история)** | Видение, принципы данных, детали F1–F10, идеи-пул, кандидаты из Summit, фазировка, журнал решений |
| [user-scenarios.md](user-scenarios.md) | **Сценарии** | 11 пользовательских сценариев S1–S11 с привязкой к фичам/фазам и сводкой «что оживает по фазам» |
| [worklog.md](worklog.md) | **Devlog** | Журнал: проблема/идея → сделано → проверено → сюжет для публичной заметки |
| [asr-bench.md](asr-bench.md) | **Качество ASR** | `localvox-bench`: WER/CER на эталонных наборах (Golos, Russian LibriSpeech), свип по окнам, сборка long-form; измеренные числа и как перемерить перед релизом |
| [uat-checklist.md](uat-checklist.md) | **Приёмка** | Ручной чек-лист: трей, автоварка, веб-архив, плеер, поиск, голос, устойчивость |
| [integrations.md](integrations.md) | **How-to** | Слоты и интеграции: настройка slots.toml (files/папки/MCP), localvox-note |
| [local-models.md](local-models.md) | **Модели** | Локальные модели по задачам (ASR, LLM, TTS, embeddings, диаризация, VAD, wake-word) с рекомендациями под наш стек |
| [architecture-target.md](architecture-target.md) | **Архитектура** | Слои ядро/модули/клиенты, целевая топология, крейты, layout данных, конфиг, API-поверхность, кроссплатформенность |
| [mobile-companion.md](mobile-companion.md) | **Мобильный** | Ярусы 0 (vault-синк) / 1 (PWA) / 2 (натив), удалённый доступ без облака |
| [positioning.md](positioning.md) | **Деньги/ниша** | Киллер-дифференциаторы, ценовой ландшафт, 4 варианта монетизации, нарративы |
| [competitors/summit-ai-notes.md](competitors/summit-ai-notes.md) | **Конкуренты** | Глубокий разбор Summit AI Notes + gap-анализ |
| [competitors/landscape-2026.md](competitors/landscape-2026.md) | **Конкуренты** | Ландшафт ниши: Granola, Meetily, Hyprnote, Screenpipe, superwhisper и выводы |
| [service-architecture.md](service-architecture.md) | Историческая схема | Прежняя топология; актуальное исполнение — architecture.md |

Исторический порядок чтения для продуктового контекста: landscape → summit → positioning → feature-backlog →
architecture-target → local-models → mobile-companion.
