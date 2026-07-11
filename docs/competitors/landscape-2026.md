# Ландшафт личных AI-ассистентов записи и заметок (июль 2026)

> Дополнение к `summit-ai-notes.md`. Цель: понять, что предлагают соседи по нише,
> где ценовой коридор и какие киллер-ниши не заняты. Обновлено 2026-07-10.

## Обзор игроков

| Продукт | Цена | Локальность | Платформы | Суть / киллер-фичи |
|---|---|---|---|---|
| **Summit AI Notes** | $12.99/мес, $149 lifetime | Локально (облако опц.) | macOS | Meeting notes + диаризация + локальный LLM; подробно в `summit-ai-notes.md` |
| **Granola** | Free (25 заметок), $14/мес | Захват локальный, **обработка облачная** | macOS, Windows | «AI-блокнот»: ты пишешь тезисы во время встречи, LLM сшивает их с транскриптом; Recipes (шаблоны выхода); **MCP-доступ к контексту встреч** из Claude/ChatGPT; интеграции Notion/Slack/HubSpot/Zapier |
| **Meetily** ⚠️ ближайший к нам | OSS MIT (free) + PRO-тариф | 100% локально: Parakeet/Whisper + диаризация + Ollama | Windows, macOS | **Тоже Rust**, 20k+ звёзд GitHub. Live-транскрипция, self-hosted. PRO: шаблоны, экспорты, автодетект встреч. Planned: календарь, чат с заметками, мобильные приложения |
| **Hyprnote** | OSS (free), BYOK | Локально (Whisper + свой HyprLLM) | macOS-first | Локальный «AI-блокнот» в стиле Granola |
| **Screenpipe** | Source-available, ~$400 lifetime | 24/7 экран+аудио локально | macOS, Windows, Linux | Тотальная память: экран (OCR) + аудио, поиск по всему; плагины-«pipes», CLI, коннект к агентам. YC S26 |
| **superwhisper** | $9.99/мес, $849 lifetime (подняли с $249) | Локальные модели (Whisper, Parakeet) | macOS, Windows, iOS | Диктовка: per-app режимы, одна лицензия на все платформы |
| **Wispr Flow** | $15/мес | **Облако полностью** | macOS, Windows, Android | Полированная диктовка с AI-cleanup; free-tier 2000 слов/нед |
| **Otter / Fireflies / tl;dv** | ~$10–20/мес | Облако + боты в звонках | web | Корпоративные нотетейкеры — антипример для нашей ниши (блог Summit целиком построен на их юридических рисках) |
| **Rewind → Limitless** | — | Было локально → облако → **куплен Meta (12.2025), Mac-приложение убито** | — | Поучительная история: облачный «second brain» умирает вместе с вендором |

## Что из этого следует

**Ценовой коридор.** Подписки $9–15/мес; lifetime от $149 (Summit) до $849 (superwhisper)
и $400 (Screenpipe). Наш ориентир «небольшие деньги» — недорогой lifetime или
open-core (ядро бесплатно, платные модули-удобства) — вписывается снизу коридора и
бьёт по больному месту всех подписочных.

**Незанятые ниши (наши киллер-фичи):**
1. **Русский first-class полностью офлайн** — ни один игрок не делает ru хорошо
   локально (Whisper/Parakeet на русском заметно слабее GigaAM v3).
2. **Голосовой интерактив** (wake-word «запиши», слоты, TTS-ответ) — нет ни у кого
   в нише; ближайшее — диктовочные тулы, но они не ассистенты.
3. **Linux** — только Screenpipe; «Linux + русский + встречи» — пусто.
4. **Ingestion YouTube/файлов** тем же конвейером — нет ни у кого из meeting-тулов.
5. **MCP-сервер поверх архива** — Granola доказала спрос (контекст встреч в
   Claude/ChatGPT); у нас это дёшево: jsonl + tantivy → инструменты
   `search_transcripts`/`get_summary` для любого внешнего агента.
6. **Открытые форматы без lock-in** (jsonl/markdown, «данные greppable») +
   durable-конвейер — инженерная надёжность как фича.
7. **Нарратив**: «Rewind купили и убили — локальные файлы не умирают вместе с
   вендором». Модульность и «жизнь без облака» — то, что подписочные игроки
   предложить не могут структурно.

**Угрозы.** Meetily — открытый, бесплатный, на Rust, с комьюнити и роадмапом в нашу
сторону (мобилки, календарь, чат). Отстройка от него: русский, голосовой интерактив,
YouTube-ingestion, CLI/API-скриптуемость, durable-ядро, Linux.

## Источники

[Granola](https://www.granola.ai/) · [цены Granola](https://get-alfred.ai/blog/granola-pricing) · [Meetily (GitHub)](https://github.com/Zackriya-Solutions/meetily) · [meetily.ai](https://meetily.ai/) · [Hyprnote vs Meetily](https://openalternative.co/compare/hyprnote/vs/meetily) · [Screenpipe (GitHub)](https://github.com/screenpipe/screenpipe) · [Screenpipe vs Limitless](https://screenpipe.com/blog/screenpipe-vs-limitless-2026) · [superwhisper vs Wispr Flow](https://spokenly.app/blog/wispr-flow-vs-superwhisper-vs-macwhisper) · [цены диктовки 2026](https://weesperneonflow.ai/en/blog/2026-04-04-ai-dictation-pricing-per-hour-vs-monthly-subscription-2026/)
