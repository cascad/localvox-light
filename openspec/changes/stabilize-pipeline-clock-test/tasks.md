# Tasks

## 1. Pipeline timing

- [x] 1.1 Воспроизвести пропуск точных 5 с при задержке runner, стабилизировать clock только этого сценария; сохранить assertions/PNG и получить red→green на одинаковом probe, записать evidence.
- [x] 1.2 Для того же дефекта часов в voice-grouping сценарии на hosted 99bd709 воспроизвести задержку, остановить clock после navigation и сохранить проверки 5/7/3 с; выполнить red→green probe и записать отдельное evidence без переписывания прежних результатов.

## 2. Integration

- [x] 2.1 Выполнить UI build/unit/полные e2e без обновления PNG, strict main/change/check-openspec/actionlint/diff; обновить hosted evidence/coverage и закоммитить проверенный этап.
- [x] 2.2 После voice-grouping fix прогнать UI build/unit/полные 81 e2e и повторы двух timing сценариев, strict change/main/check-openspec/actionlint/diff, записать границы hosted и закоммитить только этот этап.
