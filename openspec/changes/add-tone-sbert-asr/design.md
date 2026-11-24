# Design

## Context

См. proposal. `AsrEngine` принимает f32/16k и возвращает текст. `ModelPackage`
владеет файлами/построением движка, `Selection` — content identity, `AsrRuntime`
— тёплой моделью. GigaAM mel адаптер непригоден для raw PCM/state T-one.
Setup использует typed Selection/ModelKind/Component, но GigaAM сейчас обязательна
и зашита в нескольких местах. Manifest и resolver общие для runtime/doctor/download.

## Goals / Non-Goals

Локальный CPU без NVIDIA/Triton/Python на компьютере пользователя; отдельные
модули распознавания и оформления текста. Сохранить GigaAM default, старый архив,
ранний кеш, конечные retry и строгие sample boundaries.
Beam/KenLM не входит в малый первый стек и не приравнивается к greedy.

## Decisions

- Небольшой typed ASR choice доступен без inference feature: GigaAM/T-one,
  parse/serde/settings, общий resolver. Новые сессии сохраняют выбор; legacy —
  GigaAM. Явная recook меняет выбор под существующим session mutation ownership,
  после ready-проверки. Глобальный default не меняет recipe старого архива.
- T-one получает собственный `AsrEngine`: anti-aliased 16→8 kHz, 300 ms frames,
  300 ms padding с обеих сторон, f16 recurrent state, greedy CTC с collapse через
  границы frames. State сбрасывается для каждого независимого аудио-окна.
  Жёсткая проверка tensor names/shapes/dtypes; времена берутся из sample windows.
- SbertPuncCase — отдельный processor, официальный safetensors/tokenizer/config.
  Он доступен после любого ASR, включая GigaAM/T-one/Vosk, и не дублирует уже
  имеющиеся конечные знаки. Это независимый выбор, выключенный по умолчанию.
  Проверить native BERT token-classification через Candle CPU с parity к официальному
  Transformers до интеграции. Общий tokenizer first-subword/word boundaries,
  word-safe split при 512; label 0/неизвестный — ошибка, без выдуманного результата.
  Отдельная исходная ASR-версия и производный текст; processed-cache отделяется
  от акустического результата, изменение пунктуации не требует нового ASR.
  В финальной очереди это отдельные `JobPhase::Punctuation` / `Stage::Punctuation`
  с receipt `punctuated`, собственными временем и конечным retry. Worker protocol 4
  не позволяет принять старый процесс как совместимый с новой фазой.
  Ранний snapshot хранит исходные строки и identity процессора; ошибка сохраняет
  проверенный raw snapshot и акустический checkpoint, затем возвращает отказ.
  На Stop финальная пунктуация повторно использует отдельный проверенный text cache.
- Manifest добавляет независимые optional components tone/sbert. Проверка выбранного
  стека определяет обязательность. Setup/vox setup повторно используют готовые
  директории, progress/hash/staging и pending-install plan.
  Recommended по-прежнему GigaAM; custom позволяет запасной ASR и пунктуацию.
- UI/TUI используют общий settings API. Пунктуация выключена по умолчанию;
  выбор модели не подразумевает сводку/очистку LLM. Журнал показывает реальную
  модель, операцию и длительность, не объявляет success без артефакта.

## Risks / Trade-offs

Greedy хуже beam → явное обозначение и сравнительный отчёт, без смены default.
FP16/state и PCM resampling → parity tensor/decoder и настоящий pinned ONNX.
Нативная пунктуация отличается от upstream → logits/labels parity до публикации;
не заменять модель самодельными эвристиками.
Лишний recook архива → per-session choice и regression выбора/legacy/manual retry.
Две модели и CPU → один активный ASR, bounded threads, последовательные model tests.

## Migration Plan

Добавить runtime и проверить модели отдельно; затем session choice/derived artifacts;
после этого doctor/installer/settings. Старые metadata/pending-install Selection
получают совместимые defaults. Поведение фиксируется в user-guide/evidence,
регресс — в общем локальном runner. Аппаратные Mac/Linux и качество на длинном
аудио отмечать отдельно от Windows synthetic/native inference.
