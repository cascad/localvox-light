# Локальные модели для наших задач (снимок: июль 2026)

> Критерии отбора: русский язык first-class, работает на скромном железе (CPU или
> средняя GPU), внятная лицензия, вписывается в наш стек. Ключевое про стек: у нас
> уже есть onnxruntime (`ort`) — всё ONNX-семейство (ASR, VAD, KWS, embeddings,
> диаризация, TTS) едет на одном рантайме без новых зависимостей; LLM — единственный
> класс, который идёт через внешний сервер (Ollama / OpenAI-compatible, см. F1).
> Привязка к фичам — `docs/feature-backlog.md`.

## 1. ASR (транскрипция)

Сейчас: Vosk ru 0.42 (realtime, партиалы, без пунктуации) + GigaAM v3 e2e CTC int8
(батч, с пунктуацией) — уже в проекте.

| Модель | Размер | Что даёт | Заметки |
|---|---|---|---|
| **GigaAM v3** (Сбер) | ~240M | SOTA по русскому среди открытых; e2e-вариант — пунктуация и нормализация из коробки | Уже используем. MIT, ONNX. В side-by-side против Whisper large-v3 выигрывает ~70:30. Есть CTC/RNNT — RNNT можно чанк-стримить |
| Whisper large-v3-turbo | 809M | 100 языков | Путь для не-русского контента (faster-whisper / whisper.cpp — отдельный рантайм). На русском хуже GigaAM, склонен галлюцинировать на шуме |
| **T-one** (Т-Банк) | 71M | Потоковый русский ASR с низкой задержкой | Apache 2.0. Заточен под телефонию 8 kHz — для десктоп-встреч проверять; интересен как лёгкая streaming-дорожка |
| Parakeet TDT v3 / Canary (NVIDIA) | 0.6–1B | Мультиязычные (ru входит) | NeMo/onnx; кандидаты, если пойдём в мультиязычность |
| Vosk small ru | ~45MB | Дешёвые streaming-партиалы | Старый Kaldi, но партиалы нужны для TUI-live и wake-word |

**Рекомендация — двухъярусная схема:** лёгкая streaming-модель на mic-канале для
партиалов, live-вида и wake-word (Vosk small ru сейчас; T-one/стриминговый зипформер —
проверить) + **GigaAM v3 как финальный проход сегментов** (везде, включая realtime-путь,
а не только `localvox-asr`). Это заодно снимает проблему «Vosk без пунктуации».

## 2. LLM (F1: cleanup, суммаризация; F5: роутинг; F4: RAG-чат)

Подключение — через Ollama / OpenAI-compatible (решено). Ярусы по железу:

| Железо | Кандидаты | Комментарий |
|---|---|---|
| ≤16 ГБ RAM, CPU | **gpt-oss-20b** (MoE, ~3.6B актив., влезает в 16 ГБ), Qwen3-8B/4B-2507, Gemma 3 4B/12B, YandexGPT5-Lite-8B (ru, своя лицензия), T-lite | Для cleanup и коротких сводок хватает; gpt-oss-20b — неожиданно быстрый для своего класса |
| 24–32 ГБ / средняя GPU | **Qwen3-30B-A3B-Instruct-2507 — базовая рабочая лошадка** (MoE, быстрый даже частично на CPU, отличный русский, Apache 2.0), Gemma 3 27B, Mistral Small 3.2/4, **T-pro 2.0** (32B на базе Qwen3, ru-специализация + reasoning, Apache 2.0) | Наш целевой ярус для качественных summary |
| 48+ ГБ / серьёзная GPU | gpt-oss-120b, GLM-4.x-Air/GLM-5-класс (MIT), Llama 3.3 70B, GigaChat 3.1 Lightning/Ultra (открытые веса, MIT) | Если появится железо — но для наших задач избыточно |

Свежие линейки 2026 (Qwen 3.5, Gemma 4, Mistral Small 4, DeepSeek V3.2) — смотреть
на момент выбора; ландшафт меняется каждые пару месяцев.

**Рекомендация:** дефолт — Qwen3-30B-A3B (или gpt-oss-20b/Qwen3-8B на слабом железе);
русскоязычные челленджеры для бенча — T-pro 2.0, YandexGPT5-Lite-8B.
**Первый шаг — бенч на своих данных:** прогнать `transcript_uat.txt` через 3–4
кандидата на трёх задачах (cleanup, summary, action items) и сравнить с ручными
`transcript_uat_processed.md` / `transcript_uat_summary.md` — у нас уже есть готовый
эталон.

## 3. TTS (F2: голосовые подтверждения)

| Модель | Качество ru | Скорость | Лицензия |
|---|---|---|---|
| **Piper** | Хорошее (ru-голоса: irina, dmitri…) | Мгновенно на CPU | MIT, **ONNX → наш ort** |
| **Silero TTS v5** | Отличное: 5 голосов, авто-ударения, омографы, SSML, вопросная интонация (v5_4_ru) | Realtime на CPU (v5 в 3–4 раза быстрее v3) | ⚠️ Некоммерческая (CC BY-NC); коммерческое использование — платная лицензия |
| F5-TTS (ru-файнтюн) | Клонирование голоса, естественность | RTF 0.14 на RTX 3090 — нужна GPU | MIT (файнтюны проверять) |
| XTTS-v2 (Coqui) | Клонирование, ru есть | Тяжелее | CPML (некоммерч.) |
| Windows SAPI | Посредственное | Мгновенно | Встроено в ОС — zero-dep fallback |

**Рекомендация:** для коротких подтверждений «Записал в идеи…» — **Piper** (ONNX,
MIT, ноль задержки); Silero v5 — если качество голоса станет важным и лицензия
приемлема; F5-ru — дальняя опция «приятного голоса ассистента».

## 4. Embeddings + reranker (F4: семантический поиск; F5: контекст)

| Модель | Профиль |
|---|---|
| **bge-m3** | Мультиязычный стандарт, dense+sparse гибрид, есть в fastembed-rs (Rust, ONNX) — самый короткий путь |
| Qwen3-Embedding-0.6B/4B | Мультиязычный SOTA-класс (100+ языков), instruction-aware; умеет отдаваться через Ollama |
| FRIDA (SberDevices) | Русскоязычный топ ruMTEB, T5-энкодер |
| GigaEmbeddings (Сбер) | ruMTEB ~69.1, топ на конец 2024 |
| USER2 / USER-bge-m3 (deepvk) | ru-тюны bge |
| Reranker: bge-reranker-v2-m3, Qwen3-Reranker-0.6B | Второй этап точности для RAG |

**Рекомендация:** старт — bge-m3 через fastembed-rs (в наш стек без трения); при
настройке качества замерить FRIDA / GigaEmbeddings / Qwen3-Embedding на своих
транскриптах.

## 5. Диаризация + speaker ID (фаза D; live speaker ID — как beta у Summit)

| Вариант | Профиль |
|---|---|
| **sherpa-onnx pipeline** | pyannote segmentation-3.0 (onnx) + speaker embeddings (3D-Speaker ERes2NetV2 / WeSpeaker / NeMo) + кластеризация — целиком ONNX, язык-независимо, наш ort |
| pyannote community-1 (pyannote.audio 4.0) | Свежайшее открытое качество, но Python/torch — только как внешний процесс-плагин |
| NVIDIA Sortformer v2 | **Потоковая** диаризация (Interspeech 2025) — кандидат на live speaker ID |

Персистентные имена спикеров (киллер-фича по отзывам Summit): enrolled-профили
голосов + косинус по speaker embeddings — в sherpa-onnx есть готовый speaker-id.
Важно: у нас mic/sys уже разделены физически — диаризация нужна в основном **внутри**
loopback-канала («кто из собеседников»), это упрощает задачу.

## 6. VAD (сегментация — фундамент пайплайна)

Сейчас — webrtc-vad: GMM-класс ~2016 года, самое слабое звено (noise gate на нём
режет данные безвозвратно, см. «карантин» в бэклоге F3).

| Вариант | Профиль |
|---|---|
| **Silero VAD** | Стандарт де-факто, MIT, ONNX ~2 МБ, сильно точнее webrtc |
| TEN-VAD | Новее (ONNX открыт с июня 2025): по их бенчам точнее и легче Silero, лучше ловит короткие паузы | 

**Рекомендация:** заменить webrtc-vad → Silero VAD на ort (дёшево, большой выигрыш
качества сегментации и меньше ложных «шумовых» отбросов); TEN-VAD — проверить
лицензию и сравнить на наших записях.

## 7. Wake-word / KWS (F2)

| Этап | Вариант |
|---|---|
| Старт | **Матчинг в партиалах Vosk** — нулевая стоимость, русский, уже в пайплайне |
| Апгрейд | **openWakeWord** — ONNX, кастомное слово тренируется за ~час в Colab на синтетике; синтетику для русского генерим своим же TTS (Piper/Silero) |
| Альтернативы | microWakeWord (embedded-класс), sherpa-onnx KWS (кастомные фразы без переобучения, но готовые модели zh/en), Porcupine (коммерческий) |

## 8. Пунктуация/ITN

Отдельная модель (sbert_punc_case_ru, silero-te) **скорее не нужна**: GigaAM v3 e2e
уже даёт пунктуацию (см. §1 — двухъярусная схема), а LLM-cleanup (F1) закрывает
остальное.

---

## Сводка рекомендаций

| Задача | Старт | Апгрейд |
|---|---|---|
| ASR realtime | Vosk small (партиалы) + GigaAM v3 (финал сегментов) | T-one/стриминг-зипформер на партиалы |
| ASR батч (youtube/файлы) | GigaAM v3 (уже есть) | Whisper turbo для не-русского |
| LLM | Qwen3-30B-A3B / gpt-oss-20b через Ollama | Бенч T-pro 2.0, свежие линейки 2026 |
| TTS | Piper (ONNX, MIT) | Silero v5 (лицензия!) / F5-ru |
| Embeddings | bge-m3 (fastembed-rs) | FRIDA / Qwen3-Embedding по бенчу |
| Диаризация | sherpa-onnx (pyannote seg + ERes2NetV2) | Sortformer v2 для live |
| VAD | Silero VAD вместо webrtc-vad | TEN-VAD по бенчу |
| Wake-word | Vosk-партиалы | openWakeWord + ru-синтетика |

Первые два практических шага из этой карты: (1) LLM-бенч на UAT-эталоне,
(2) замена webrtc-vad на Silero VAD.

## Источники

- LLM: [HF: open models to run locally 2026](https://huggingface.co/blog/daya-shankar/open-source-llm-models-to-run-locally), [TECHSY leaderboard 07.2026](https://techsy.io/en/blog/best-open-source-llms-2026), [klymentiev.com: best local LLM by hardware](https://klymentiev.com/blog/best-local-llm), [сравнение отечественных LLM](https://azoneai.ru/blog/10-sravnenie-llm/), [обзор российских нейросетей 2026](https://vc.ru/aihub/2842998-obzor-rossiyskih-neyrosetey-2026-goda)
- ASR: [GigaAM (GitHub)](https://github.com/salute-developers/GigaAM), [T-one pipeline](https://www.blog.brightcoding.dev/2026/06/01/t-one-the-high-performance-russian-asr-pipeline-developers-love), [Northflank: open STT 2026](https://northflank.com/blog/best-open-source-speech-to-text-stt-model-in-2026-benchmarks), [WisprFlow vs Whisper vs GigaAM](https://news.hamidun.com/en/news/7078/wisprflow-whisper-and-gigaam-who-recognizes-russian-english-)
- TTS: [silero-tts v5 (Хабр)](https://habr.com/ru/articles/961930/), [v5 вопросная интонация](https://habr.com/ru/articles/1015942/), [обзор open-source ru TTS](https://vc.ru/ai/2641014-obzor-open-source-modeley-tts-dlya-sinteza-rechi-na-russkom-yazyke)
- Embeddings: [ruMTEB paper](https://arxiv.org/pdf/2408.12503), [GigaEmbeddings](https://arxiv.org/pdf/2510.22369), [Qwen3-Embedding](https://github.com/QwenLM/Qwen3-Embedding)
- Диаризация: [pyannote community-1](https://www.pyannote.ai/blog/community-1), [sherpa-onnx diarization](https://huggingface.co/csukuangfj/sherpa-onnx-apk/blame/dc0adc4dee002b4755741826639b52344d545679/generate-speaker-diarization.py), [pyannote vs Sortformer](https://vast.ai/article/whisper-pyannote-sortformer-diarization-vast)
- VAD: [TEN-VAD](https://github.com/TEN-framework/ten-vad), [Silero VAD](https://github.com/snakers4/silero-vad)
- Wake-word: [openWakeWord](https://github.com/dscripka/openWakeWord), [обзор wake-word 2026](https://picovoice.ai/blog/complete-guide-to-wake-word/)
