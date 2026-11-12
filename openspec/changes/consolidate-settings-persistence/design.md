# Design

## Context

См. proposal. `env_file` уже владеет редактированием `.env`, но API использует
прямую запись, а setup дублирует read/check/atomic publish. CFG-001 требует
сохранить остальные настройки и не применять изменения при ошибке сохранения.

## Goals / Non-Goals

Один владелец read-modify-write. Сохранить `save`/`save_to`, формат `.env`, HTTP DTO,
настройки restart/live и plan/confirm setup. Не менять renderer, не вводить
новый storage/framework, background service или глобальную транзакцию.

## Decisions

- Immutable `env_file::Snapshot` связывает путь и прочитанное содержимое. Missing
  отличается от пустого файла; read errors не превращаются в пустую конфигурацию.
  Setup проверяет snapshot перед действиями и сохраняет через него в конце.
- `save_to` сериализует read→edit→publish процессным mutex. `Snapshot::save`
  проверяет исходное состояние под тем же mutex; UI edits не затирают друг друга
  внутри daemon. Не обещать CAS против произвольного стороннего процесса.
- Публикация использует существующий `artifacts::atomic_write` и его Windows
  bounded retry. Отдельный самописный rename/retry или второй редактор не нужны.
- Existing symlink path разрешается до целевого файла; проверяется и содержимое,
  и destination, чтобы сохранить действующий link. Не заменять dangling link новым
  `.env`: сообщить ошибку. Настоящие OS permissions сохраняют свои ограничения.
- Если `apply` не меняет parsed assignments, не публиковать файл: остаются исходные
  комментарии/quotes/line endings и mtime. Порядок нескольких edits одного ключа
  определяется прежним `apply`, не фильтрацией каждого edit по старому значению.
- При проверке no-op обнаружен дефект чтения quoted value с inline comment:
  закрывающая кавычка перед `#` не распознавалась. Ограниченное исправление в
  существующем `unquote` сохраняет hash внутри quotes; второй parser не вводится.

## Risks / Trade-offs

- Сохранение изменённых строк и экранирование → прежние env_file tests, HTTP и setup.
- Отказ atomic replace на Windows → настоящий held handle, прежние bytes остаются,
  настройки в окружении не применяются; временный файл убирается библиотекой.
- Потеря independent edits → concurrent save_to fixture с разными ключами.
- Snapshot stale / missing-to-empty → явные tests, ошибка до записи.
- Symlinks / Unix permissions → portable cfg tests где платформа позволяет;
  native Mac acceptance не объявлять выполненной на Windows.

## Migration Plan

Сначала общий writer и существенные core tests, затем переключить setup с прежними
ownership/confirmation. Прогнать settings HTTP и setup integration; release build,
evidence и commit. Миграции файлов нет; пользовательский `.env` не трогать.
