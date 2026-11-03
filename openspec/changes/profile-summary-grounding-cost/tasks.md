# Tasks

## 1. Численный профиль существующей проверки

- [x] 1.1 Добавить opt-in компоненты к общей реализации grounding, сохранив штатный путь без измерений; model-free tests должны подтвердить точную parity с/без NER, числа/пустой ответ, кеш и отсутствие входного текста/имён в serialized timings.
- [x] 1.2 Реализовать инженерный localvox-grounding-profile: явные входы, новые output/partial reports, фактические NER counters/ошибки и fingerprints; проверить privacy, отказ overwrite и отказ NER unit tests, обновить CLI-документ и CI.

## 2. Reference без генерации LLM

- [x] 2.1 Собрать release probe и выполнить два bounded fresh-process профиля на прежних публичных STT/готовой сводке с существующим GLiNER; сверить SHA входов/модели, cold/warm components, фактический reuse и parity, сохранить численные evidence с unknown и пределами.
- [x] 2.2 Обновить coverage LLM-004/PIPE-007, каталог и текущий статус proposal; пройти targeted Rust, strict main/change, check-openspec и diff, закоммитить проверенный этап без переключения настроек/моделей/рабочего демона.
