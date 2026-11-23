# Tasks

## 1. Контекстное удаление

- [x] 1.1 Привязать d/Delete в Archive/Notes и исправить цель палитры для списка/деталей, ограничить заголовок; state tests для фильтра, captured identity, подтверждения/отмены/ввода, RU profile и пустого контекста; обновить US-023/030 и подсказки.
- [x] 1.2 Обрабатывать acknowledgement по captured identity, ограничить выбор и отсечь старые чтения epoch; state tests success/error/stale/несовпадающий ответ и изменение выбора.

## 2. Интеграционные проверки

- [x] 2.1 Проверить production HTTP удаление записей/заметок в частных файлах с сохранением соседа; delayed DELETE через I/O с доступным UI/control и настоящий PTY/ConPTY d/палитра→Esc/s→обновление.
- [x] 2.2 Прогнать затронутые suites и native build, проверить snapshots, rustfmt/diff и strict OpenSpec; записать evidence и честную границу Mac проверки, подготовить завершённый этап к коммиту.
