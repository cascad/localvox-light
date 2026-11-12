# Design

## Context

См. proposal. independent TUI уже умеет bounded compatible/connect-refused bootstrap.
Main CLI зависит от capture DLLs, а shell aliases не сохраняют одинаковую семантику
во всех Windows shells. Нужен небольшой native entry без модели в зависимостях.

## Goals / Non-Goals

Единый запуск клиента и нормальная установка команды, без новых правил владения
демоном. Полное завершение приложения обсуждается отдельно и не вводится молча.

## Decisions

- Второй binary vox в terminal crate; общий entry вместо дублирования main.
  vox загружает соседние presets/.env и фиксирует свой cwd, затем использует тот
  же client/runtime/launcher. Raw localvox-tui сохраняет прежний cwd и defaults.
- Отдельный модуль регистрации: Windows user PATH через HKCU Environment,
  Unix Bash/Zsh/Fish startup config. Новая команда указывает на каталог установки,
  поэтому сохраняется соседство daemon/worker и не нужны wrappers или копии DLL.
- Native `--install-command` / `--check-command` выполняются до TTY/API/model
  работы. Installer вызывает регистрацию; `NoPathUpdate`/`--no-path-update`
  позволяет portable install и изолированные CI tests без изменения реального HOME/PATH.
- Проверка чужой vox до записи. Профили: snapshot и проверка перед atomic replace,
  сохранение прав/ссылок, только один managed block; повтор — no-op. Не обещать
  глобальный CAS против произвольных сторонних редакторов.
- Windows изменение registry value тестируется на отдельном test key, не на
  настоящем user PATH. Unix edit planner и shell execution — в temp HOME.

## Risks / Trade-offs

- PATH в уже открытом терминале старый → явное сообщение открыть новый terminal;
  Windows отправляет bounded environment-change notification.
- Неизвестный shell/чужой managed block → ничего не затирать, объяснить ручной PATH.
- Native Mac не доступен здесь → portable tests отдельно от аппаратной приёмки.
- Нативный vox добавляет небольшой executable; без shell-escaping аргументов и
  отдельного capture stack. Packaging/CI проверяют полный набор binary файлов.
