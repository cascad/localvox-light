# Tasks

Граница выпуска подтверждена владельцем 2026-11-10: ранняя транскрибация готовых
кусков в UI и TUI → просмотр владельцем → баги → отдельный рефакторинг.

| Результат для проверки | Состав | Состояние |
|---|---|---|
| R1 · Готовые куски распознаются, Stop завершает полную запись с reuse | 1.1–3.2 | Готово: worker/crash fixtures и native 150 s smoke прошли; reference performance в R4 |
| R2 · Очередь, настройка и отмена работают во время записи | 4.1–4.2 | Готово: real coordinator stop/disable/cancel/Retry; default gate в R4 |
| R3 · Готовое можно читать, копировать и слушать в UI/TUI без прыжков | 4.3, 6.1–6.3 | Готово: HTTP/оба клиента/ConPTY, native mpv и FLAC; hardware отдельно |
| R4 · Кандидат проходит общий прогон и запускается по короткой инструкции | 7.1–7.3 | Готово 2026-11-11: grouped cache; полные legacy/current R3 6/6 пар прошли, default on; регрессии и новый комплект проверены |

После R4 передать кандидата владельцу и остановиться на проверке 7.4. Новые фичи,
подбор моделей и общий рефакторинг не добавлять к этому выпуску. Далее — баги по
результатам просмотра; аппаратные сценарии не объявлять выполненными заранее.

## 1. Confirmed capture input

- [x] 1.1 Реализовать generation/sealed sample ranges/terminal capture manifest (STORE-004), не публиковать ошибки close/sync/rename; проверить fault injection каждого commit, пустой файл, pre-roll/pause/Stop/две дорожки и interrupted recovery; обновить описание первичных артефактов.
- [x] 1.2 Ввести согласованный chunk reader/converter lease и bounded supervised FLAC с sample-count/PCM проверкой до удаления WAV (STORE-002/RV-005); проверить concurrent read/convert, short-success FLAC, timeout/shutdown/no-ffmpeg и сохранение исходника; записать границы проверки RV-005.

## 2. Continuous window planning

- [x] 2.1 Выделить общий offline/incremental planner без изменения рецепта разрезания; property/fixture проверками доказать одинаковые PCM windows при случайной нарезке, слове на стыке файлов, тишине, forced cut, неполном frame и только настоящем EOF.
- [x] 2.2 Подключить capture-owned лёгкий planner вне callback: только новые sealed ranges, coalesced disk notifications, bounded decoder/RAM, capture lease до окончания всех писателей; проверить отказ planner, pause/Stop/shutdown без новых PCM, delete после Stop, uneven/отключённые/поздние источники и safe blocking общей границы; обновить архитектуру ownership/времени.

## 3. Durable ASR work and finalization

- [x] 3.1 Реализовать immutable descriptors/window cache/checkpoint и отдельные per-source/common frontiers (PIPE-015/016); тестами с остановкой между result/sync/checkpoint доказать отсутствие дыр/дублей, учёт empty outcomes, отказ битого кеша/изменённого PCM/model/recipe и повтор только недостающего.
- [x] 3.2 Дать worker конечный input horizon и общий путь принятия кеша в full cook, EOF tail, final diarization и обычные версии/receipts; настоящим worker с model-free ASR fixture проверить crash каждого шага final commit, source/parent/timing, legacy fallback, запрет summary/index до полного подтверждения и отсутствие повторного ASR принятых окон; документировать recovery/rollback.

## 4. Coordinator, controls and API

- [x] 4.1 Добавить coalesced incremental intent, новый worker protocol/preflight и finite задания в существующих ресурсных линиях без starvation обычной очереди (PIPE-018); real-coordinator fixture проверяет лимиты, отсутствие spawn без новых descriptors, повторные уведомления, Stop/claim race, несовместимый worker и отсутствие длительного I/O под queue mutex; обновить protocol docs.
- [x] 4.2 Реализовать capture/processing mutation leases и различие Stop, Cancel, Retry и выключения раннего режима; проверить cancel при продолжающемся capture, sticky отмену после Stop/restart, отсутствие удаления живым planner, конфликты языка/best/recook, включённый/выключенный autocook и видимую Live предустановку нового bool в UI/TUI settings schema.
- [x] 4.3 Добавить согласованный partial/final snapshot API, stable line IDs и неизменяемые audio revision URLs/Range/peaks (ARCH-006); настоящим HTTP проверить stale revision, повреждённый chunk без fake silence, точное аудио через шов, рост capture, доступность старого playing snapshot и отказ полного export до final; описать API/доступные границы.

## 5. Deferred text precomputation

По решению владельца 2026-11-10 ранняя LLM-правка вынесена за границу выпуска.
Прежние задачи 5.1 (общий planner/cache точных refine/cleanup запросов) и 5.2
(раннее применение/финальные parent receipts), ID PIPE-017, остаются невыполненными
идеями для отдельного последующего согласования. Их исключение из текущего счётчика
не означает реализацию. Существующая правка после Stop сохраняется без переделки.

## 6. UI and TUI behavior

- [x] 6.1 Показать partial Реплики/Текст/границы/журнал (PIPE-011, ARCH-006) в UI, сохранить copy, stable selection/scroll и playing snapshot; browser unit/e2e и visual проверки должны покрывать несколько обновлений, выделение, старый HTTP ответ, ошибку, Stop→final и отсутствие прыжка scroll/seek/фокуса.
- [x] 6.2 Подключить тот же snapshot/state к TUI: устойчивый anchor/selection, цельный Текст с одним copy hotkey, прежние player hotkeys и статусы ожидания; snapshot/ConPTY + real API/null-audio mpv проверки покрывают растущий документ, late/stale revisions, Stop/Cancel и отсутствие сброса проигрывания.
- [x] 6.3 Обновить затронутые US-ID в user-guide вместе с реально доступными сценариями: готовая часть, что копируется, граница аудио, разные Stop/Cancel и настройка; проверить общим temporary API→UI/TUI сценарием без ручного редактирования архивных файлов, будущие действия не выдавать за реализованные.

## 7. Integration and acceptance

- [x] 7.1 Прогнать единую fault/restart серию с синтетическим producer и реальными coordinator/worker/API: VAD seam, тишина, lost/empty chunk, disk refusal, decoder/ASR timeout, две дорожки, FLAC race, смена входа и crash на финализации; сохранить input/output receipts и воспроизводимые assertions без микрофона/пользовательского архива.
- [x] 7.2 Выполнить alternating offline/incremental native серии на сохранённом публичном двухчасовом PCM: сравнить окно за окном PCM hashes/coverage, ASR cache calls, final text differences, wall/active CPU/I/O/RAM/startup и Stop→ready; проверить reference-budget из design, при провале не включать default и не менять пороги/алгоритм ради результата.
- [x] 7.3 После автономных gate обновить evidence затронутых принятых требований и локальную карту новых IDs; включить обещанный default только для проверенного полного пути, прогнать соответствующие Rust/API/browser/ConPTY regressions и strict main/change/check-openspec/diff; коммитить проверенные этапы без push, не sync/archive невыполненное.
- [ ] 7.4 Выполнить с владельцем аппаратную проверку длительной записи microphone/loopback и слабого Mac: качество стыков, capture backlog/звук, отзывчивость, Stop и восстановление; до фактического прогона оставить not-run и не объявлять готовность на этом железе.

Текущий срез R4 от 2026-11-11: [исправление кеша, полные gates, default и регрессии](evidence/default-on-validation.md),
[новый комплект и native smoke](evidence/default-on-bundle.json).
7.3 завершён; 7.4 аппаратная приёмка остаётся not-run. Новый комплект:
`target/review/incremental-default-on`, запуск — в [короткой инструкции](../../../docs/incremental-acceptance.md).
Новые фичи/оптимизации не добавляются; change не sync/archive.

Исторический срез R4 от 2026-11-10: [fault/restart и receipts](evidence/fault-series.md),
[native reference — correctness pass, budget fail](evidence/native-reference.md),
[кандидат/регрессии/traceability](evidence/review-candidate.md),
[короткая проверка владельцем](../../../docs/incremental-acceptance.md).
В 7.3 выполнены сборка, регрессии и evidence, но обещанный default on запрещён
непройденным reference gate. Требование PIPE-018 не переписано под результат;
задача остаётся открытой до отдельного решения о lifecycle/default. Кандидат
доступен с явным переключателем для согласованного просмотра UI/TUI. Дальнейшие
самовольные оптимизации/фичи не добавляются; 7.4 not-run, change не sync/archive.
