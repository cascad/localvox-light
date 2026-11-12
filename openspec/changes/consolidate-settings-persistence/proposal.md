# Proposal

## Why

API сохраняет `.env` прямым truncate/write, а terminal setup отдельно проверяет
снимок и публикует файл атомарно. Разные write paths создают разные гарантии
сохранности, повторную запись неизменённых значений и риск потери независимых
параллельных HTTP edits. Нужен один небольшой владелец записи конфигурации.

## What Changes

- Свести snapshot/read/verify/publish в `core/env_file`; API сохраняет свои
  существующие `save`/`save_to`, setup использует тот же snapshot writer.
- Сериализовать read-modify-write внутри процесса; отказы чтения/публикации
  возвращать до применения настроек. Использовать существующий atomic writer.
- Пропускать сохранение семантически неизменённой конфигурации, сохраняя исходные
  quotes/comments/line endings. Существующий редактор изменяемых строк не менять.
- Проверить stale snapshot, параллельные независимые edits, отсутствие потери
  старого файла при Windows sharing denial и настоящий settings HTTP.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. `skip_specs: true`: внутренний рефакторинг и усиление исполнения действующих
CFG-001/002/003, SETUP-006 delta. HTTP/CLI/schema/restart policies и пользовательские
действия не меняются. Нет новой модели глобальной cross-process транзакции.

## Impact

`core/env_file`, setup executor и tests, settings HTTP regression, evidence.
Рабочая `.env`, archive/daemon/модели не используются для проверок. Новых deps нет.
Остальные большие модули и BL-006 вне этого ограниченного среза.
