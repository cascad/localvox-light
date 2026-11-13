# Proposal

## Why

На fee3103 UI/TUI и Linux прошли, Windows/macOS Intel упали на downloader fixtures.
Windows Hidden теряется при сравнении normalized и исходного пути; Mac отвергает
стандартный /var → /private/var. Windows native curl повреждает Unicode output path.

## What Changes

- Привести installer tooling к уже требуемым SETUP-001/002 и SETUP-008 в
  [исходной delta](../use-user-model-storage/specs/installation-models/spec.md).
- Сравнивать одинаково нормализованные Windows пути при создании Hidden root.
- Разрешить Unix ancestor aliases через физический parent, сохранив запрет
  замены symlink самой модели и ограничение конкретным component destination.
- Открывать .part через shell redirection, чтобы native curl не кодировал имя файла.
- Добавить regression fixtures и сохранять результаты hosted/local раздельно.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет изменений требуемого поведения. `skip_specs: true`: исправляется installer
tooling по существующему контракту, новые требования и исключения не вводятся.
Документы исходной delta и её исторические результаты не переписываются.

## Impact

PS/Bash path helpers, Bash fetch, delivery fixtures, CI evidence. Модели,
runtime/форматы, UI/TUI и пользовательские действия не меняются.
