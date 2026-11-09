# Tasks

## 1. Direct profiling and regression checks

- [x] 1.1 Сделать обычный text-only прямым подключением, сохранить явные telemetry/experimental paths; обновить docs/CI и проверить unit tests и реальные worker fixtures всех четырёх modes, включая null counters и privacy.
- [x] 1.2 Собрать release profiler, зафиксировать hashes worker/config/corpus/source и заранее выбранные review indices; подготовить новые manifests и проверить неизменность исходников до native серии.

## 2. Native quality and timing evidence

- [x] 2.1 Выполнить по три fresh repeats двух коротких corpus на Gemma E4B и Qwen 4B; сохранить строгие evaluator verdicts, отдельно useful/missed/violation/review, не менять ожидания.
- [x] 2.2 Выполнить четыре длинных cleanup в порядке Gemma/Qwen/Gemma/Qwen, проверить artifacts и заранее выбранные строки с контекстом, записать wall times, ограничения и новые риски без смены default model.
- [x] 2.3 Опубликовать content-free evidence, обновить затронутые coverage/review/catalog; проверить fingerprints, строгую валидацию main/change, check-openspec и diff; закоммитить только проверенный этап.
