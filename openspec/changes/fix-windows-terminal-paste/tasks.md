# Tasks

## 1. Безопасный ввод

- [x] 1.1 Подключить общий Input adapter, Windows VT/UTF-16 bridge и библиотечный parser; проверить split markers/Unicode, незавершённую вставку, key modifiers, resize и восстановление mode через unit и native ConPTY.
- [x] 1.2 Открывать заполненный импорт одиночного URL вне ввода, сохранив формы/поиск; проверить Enter/Esc и отсутствие побочных действий, обновить US-004 и ручную матрицу терминалов.

## 2. Интеграция и поставка

- [x] 2.1 Выполнить terminal regression/PTY, release build, strict main/change/check-openspec/diff; записать доказательства и not-run границы, установить проверенный TUI и создать отдельный коммит.
