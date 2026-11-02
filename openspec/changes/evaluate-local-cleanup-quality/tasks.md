# Tasks

## 1. Контрольный набор

- [x] 1.1 Создать расширенный synthetic corpus и evaluator ordinary cleanup с expected/missed/violation/review, подготовкой новых input и проверкой артефактов; unit tests должны ловить потерю ограничения, чисел/цитат, no-op, допустимую вариацию и повреждённые outputs; описать запуск и ограничения.

## 2. Native качество

- [x] 2.1 Выполнить Gemma E4B и Qwen 4B по три repeats неизменённого worker на corpus; сохранить hashes, actual outputs/категории/полезность и разобрать нарушения/review, не менять oracle по ответам.
- [x] 2.2 Выполнить автоматический risk scan сохранённых публичных длинных результатов и agent-review выбранных строк с контекстом, сохранить критерии/индексы/выводы; сверку с аудио и приёмку владельцем оставить not-run.

## 3. Интеграция evidence

- [x] 3.1 Обновить LLM-003/PIPE-006 evidence, RV-018 и каталог по фактическим результатам без production rollout; пройти Python tests, strict main/change, check-openspec и diff, затем закоммитить завершённый этап.
