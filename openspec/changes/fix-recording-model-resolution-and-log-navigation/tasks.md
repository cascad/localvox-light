# Tasks

## 1. Модель и наблюдаемый отказ

- [x] 1.1 Подключить общий resolver к Config; red→green для пользовательского home, независимого cwd, portable и явного пути, проверить передачу model_dir в worker transports; evidence SETUP-002/008.
- [x] 1.2 Передать execution_id в queue DTO и учитывать только новый отказ; тесты API/клиента/TUI для повторного poll, новой попытки с тем же текстом и recovery, сохранить бюджеты очереди; evidence PIPE-010/011.

## 2. Управление логами

- [x] 2.1 Добавить j/k/h/l/y для console/journal, выбранное копирование и Ctrl+Y всего лога, обновить подсказки и US-007/048; state и ConPTY проверки клавиш/ввода/clipboard task.

## 3. Проверка этапа

- [x] 3.1 Прогнать затронутые suites, native сборку, rustfmt/diff, strict change/main и check-openspec; записать границы Mac приёмки и подготовить версию v0.2.8.

После локальной проверки: коммит, push и тег v0.2.8 разрешены владельцем 2026-11-23.
