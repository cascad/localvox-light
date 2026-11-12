# Proposal

## Why

Владелец выбрал короткую команду `vox` для Windows/macOS/Linux и уточнил её
назначение: открыть TUI, запустив демон при отсутствии работающего, либо подключиться
к существующему. Команда должна появляться при установке без перезаписи чужого vox.

## What Changes

- Отдельный native `vox` использует общий entry TUI и уже проверенный bootstrap
  демона. Не зависит от LOCALVOX_LIGHT_TUI и не включает capture внутри клиента.
- Полный bundle содержит vox. Installer регистрирует каталог в пользовательском
  PATH; есть opt-out. Повторная регистрация — без повторных записей и дублей.
- Windows: user PATH без admin; Unix: поддерживаемые Bash/Zsh/Fish startup files
  с сохранением чужого содержимого и симлинков. Конфликт имён явно сообщается.
- Read-only command check, доставка, docs/user-guide/evidence и проверки в CI.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `installation-models`: SETUP-007 установка и проверка короткой команды.
- `runtime-settings`: RUN-005 запуск vox как TUI + повторное использование демона.

## Impact

Terminal entry/native executable, command registration module, installers,
packager, tests/docs. Модели и worker-протокол не меняются. Полный выход вместе
с демоном — отдельное обсуждение владельца; здесь сохраняется существующий выход
из клиента, без неявной остановки активной записи и без process-name kills.
