# Tasks

## 1. Confirmed capture input

- [ ] 1.1 Реализовать generation/sealed sample ranges/terminal capture manifest (STORE-004), не публиковать ошибки close/sync/rename; проверить fault injection каждого commit, пустой файл, pre-roll/pause/Stop/две дорожки и interrupted recovery; обновить описание первичных артефактов.
- [ ] 1.2 Ввести согласованный chunk reader/converter lease и bounded supervised FLAC с sample-count/PCM проверкой до удаления WAV (STORE-002/RV-005); проверить concurrent read/convert, short-success FLAC, timeout/shutdown/no-ffmpeg и сохранение исходника; записать границы проверки RV-005.

## 2. Continuous window planning

- [ ] 2.1 Выделить общий offline/incremental planner без изменения рецепта разрезания; property/fixture проверками доказать одинаковые PCM windows при случайной нарезке, слове на стыке файлов, тишине, forced cut, неполном frame и только настоящем EOF.
- [ ] 2.2 Подключить capture-owned лёгкий planner вне callback: только новые sealed ranges, coalesced disk notifications, bounded decoder/RAM, capture lease до окончания всех писателей; проверить отказ planner, pause/Stop/shutdown без новых PCM, delete после Stop, uneven/отключённые/поздние источники и safe blocking общей границы; обновить архитектуру ownership/времени.

## 3. Durable ASR work and finalization

- [ ] 3.1 Реализовать immutable descriptors/window cache/checkpoint и отдельные per-source/common frontiers (PIPE-015/016); тестами с остановкой между result/sync/checkpoint доказать отсутствие дыр/дублей, учёт empty outcomes, отказ битого кеша/изменённого PCM/model/recipe и повтор только недостающего.
- [ ] 3.2 Дать worker конечный input horizon и общий путь принятия кеша в full cook, EOF tail, final diarization и обычные версии/receipts; настоящим worker с model-free ASR fixture проверить crash каждого шага final commit, source/parent/timing, legacy fallback, запрет summary/index до полного подтверждения и отсутствие повторного ASR принятых окон; документировать recovery/rollback.

## 4. Coordinator, controls and API

- [ ] 4.1 Добавить coalesced incremental intent, новый worker protocol/preflight и finite задания в существующих ресурсных линиях без starvation обычной очереди (PIPE-018); real-coordinator fixture проверяет лимиты, отсутствие spawn без новых descriptors, повторные уведомления, Stop/claim race, несовместимый worker и отсутствие длительного I/O под queue mutex; обновить protocol docs.
- [ ] 4.2 Реализовать capture/processing mutation leases и различие Stop, Cancel, Retry и выключения раннего режима; проверить cancel при продолжающемся capture, sticky отмену после Stop/restart, отсутствие удаления живым planner, конфликты языка/best/recook, включённый/выключенный autocook и видимую Live предустановку нового bool в UI/TUI settings schema.
- [ ] 4.3 Добавить согласованный partial/final snapshot API, stable line IDs и неизменяемые audio revision URLs/Range/peaks (ARCH-006); настоящим HTTP проверить stale revision, повреждённый chunk без fake silence, точное аудио через шов, рост capture, доступность старого playing snapshot и отказ полного export до final; описать API/доступные границы.

## 5. Optional text precomputation

- [ ] 5.1 Выделить общий planner refine/cleanup requests и кеш полных зависимостей (PIPE-017), не менять prompt/batch/glossary/guards ради reuse; проверить закрытый/последний/oversized пакет, late glossary term, модель/параметры/source order, чужие/повторные/пропущенные IDs и текущие retention regressions.
- [ ] 5.2 Подключить явно запрошенную правку к готовому общему STT префиксу и final source/parent receipts; real-worker fixture для refine/cleanup/chain/off доказывает отсутствие LLM без запроса, fallback на source, перепроверку guards и инвалидирование только несовпавших пакетов; обновить engineering docs, не возвращать retired auto-clean toggles.

## 6. UI and TUI behavior

- [ ] 6.1 Показать partial Реплики/Текст/границы/журнал (PIPE-011, ARCH-006) в UI, сохранить copy, stable selection/scroll и playing snapshot; browser unit/e2e и visual проверки должны покрывать несколько обновлений, выделение, старый HTTP ответ, ошибку, Stop→final и отсутствие прыжка scroll/seek/фокуса.
- [ ] 6.2 Подключить тот же snapshot/state к TUI: устойчивый anchor/selection, цельный Текст с одним copy hotkey, прежние player hotkeys и статусы ожидания; snapshot/ConPTY + real API/null-audio mpv проверки покрывают растущий документ, late/stale revisions, Stop/Cancel и отсутствие сброса проигрывания.
- [ ] 6.3 Обновить затронутые US-ID в user-guide вместе с реально доступными сценариями: готовая часть, что копируется, граница аудио, разные Stop/Cancel и настройка; проверить общим temporary API→UI/TUI сценарием без ручного редактирования архивных файлов, будущие действия не выдавать за реализованные.

## 7. Integration and acceptance

- [ ] 7.1 Прогнать единую fault/restart серию с синтетическим producer и реальными coordinator/worker/API: VAD seam, тишина, lost/empty chunk, disk refusal, decoder/ASR timeout, две дорожки, FLAC race, смена входа и crash на финализации; сохранить input/output receipts и воспроизводимые assertions без микрофона/пользовательского архива.
- [ ] 7.2 Выполнить alternating offline/incremental native серии на сохранённом публичном двухчасовом PCM: сравнить окно за окном PCM hashes/coverage, ASR cache calls, final text differences, wall/active CPU/I/O/RAM/startup и Stop→ready; проверить reference-budget из design, при провале не включать default и не менять пороги/алгоритм ради результата.
- [ ] 7.3 После автономных gate обновить evidence затронутых принятых требований и локальную карту новых IDs; включить обещанный default только для проверенного полного пути, прогнать соответствующие Rust/API/browser/ConPTY regressions и strict main/change/check-openspec/diff; коммитить проверенные этапы без push, не sync/archive невыполненное.
- [ ] 7.4 Выполнить с владельцем аппаратную проверку длительной записи microphone/loopback и слабого Mac: качество стыков, capture backlog/звук, отзывчивость, Stop и восстановление; до фактического прогона оставить not-run и не объявлять готовность на этом железе.
