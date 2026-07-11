# Документация localvox — индекс

Продуктовые разрезы (все черновики живые, обновлены 2026-07-10):

| Документ | Разрез | Что внутри |
|---|---|---|
| [feature-registry.md](feature-registry.md) | **Реестр (канон)** | Зафиксированные принципы P1–P9 и фичи F1–F11 — изменения только через журнал решений |
| [work-plan.md](work-plan.md) | **План работ** | Stage 0 (замеры) + фазы A–D: пакеты работ, задачи, критерии приёмки, риски |
| [feature-backlog.md](feature-backlog.md) | **Фичи (история)** | Видение, принципы данных, детали F1–F10, идеи-пул, кандидаты из Summit, фазировка, журнал решений |
| [user-scenarios.md](user-scenarios.md) | **Сценарии** | 11 пользовательских сценариев S1–S11 с привязкой к фичам/фазам и сводкой «что оживает по фазам» |
| [local-models.md](local-models.md) | **Модели** | Локальные модели по задачам (ASR, LLM, TTS, embeddings, диаризация, VAD, wake-word) с рекомендациями под наш стек |
| [architecture-target.md](architecture-target.md) | **Архитектура** | Слои ядро/модули/клиенты, целевая топология, крейты, layout данных, конфиг, API-поверхность, кроссплатформенность |
| [mobile-companion.md](mobile-companion.md) | **Мобильный** | Ярусы 0 (vault-синк) / 1 (PWA) / 2 (натив), удалённый доступ без облака |
| [positioning.md](positioning.md) | **Деньги/ниша** | Киллер-дифференциаторы, ценовой ландшафт, 4 варианта монетизации, нарративы |
| [competitors/summit-ai-notes.md](competitors/summit-ai-notes.md) | **Конкуренты** | Глубокий разбор Summit AI Notes + gap-анализ |
| [competitors/landscape-2026.md](competitors/landscape-2026.md) | **Конкуренты** | Ландшафт ниши: Granola, Meetily, Hyprnote, Screenpipe, superwhisper и выводы |
| [service-architecture.md](service-architecture.md) | Текущее состояние | Звездообразная схема сервиса как есть |

Порядок чтения для контекста: landscape → summit → positioning → feature-backlog →
architecture-target → local-models → mobile-companion.
