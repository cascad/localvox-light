# Tasks

## 1. Состояния и оформление списка

- [x] 1.1 Реализовать компактную projection Session/совпадающей queue в цветной индикатор и подпись; проверить running/pending/waiting_input/recording/ready/failed/retry/cancel/unknown, ошибки и несовпадающие ответы unit и style tests; обновить US-006/руководство TUI в этом срезе.
- [x] 1.2 Добавить 100 ms анимацию только видимого активного списка без новых HTTP запросов; проверить кадры, фиксированные колонки/selection/scroll, темы/ASCII/no-color snapshots и настоящий ConPTY с переходом running→pending→done/error.
- [x] 1.3 Добавить прямой Unicode-фильтр списка и RU/EN command keys; проверить имя/регистр/пустой результат/вставку/refresh/Enter, сохранение открытого документа и literal form text, русские команды поиска/времени и ConPTY; обновить справочник и подсказки.

## 2. Проверка и поставка

- [x] 2.1 Прогнать terminal/client/process regressions, собрать запускаемый TUI без остановки пользовательских процессов, проверить strict main/change/check-openspec/diff; сохранить фактическое evidence/локальную строку ARCH-010 и закоммитить относящиеся файлы без push.
