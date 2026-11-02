# Tasks

## 1. Admission без переключения модели

- [x] 1.1 Закрепить RV-020 и опасные похожие замены собственными regression cases; запустить их на прежнем guard и сохранить фактический RED, рядом оставить полезные typo/согласование/ASR controls.
- [x] 1.2 Реализовать словарное основание и строгое локальное согласование, ограничить обход через group repair; закрепить зависимость и attribution/release notices, обновить краткую документацию, пройти LLM unit tests и rustfmt.
- [x] 1.3 Проверить настоящий текущий worker с fake provider для refine/cleanup: опасные словоформы отвергнуты, хорошие правки/согласование приняты, source/тайминги/родители/receipts сохранены; выполнить profiler unit и runtime-mock tests.

## 2. Полезность, стоимость и контракт

- [x] 2.1 Replay неизменённых коротких и четырёх длинных результатов через release guard без вызовов моделей; проверить RV-020, прежние RV-018/019 и 24 useful, измерить cold/warm CPU/RSS/размер бинарника, записать честные пределы в evidence.
- [x] 2.2 Синхронизировать проверенные delta, обновить US-011 и coverage/review/catalog; выполнить evaluator tests, strict main/change, check-openspec и diff, закоммитить завершённый этап без смены settings/defaults/работающего демона.
