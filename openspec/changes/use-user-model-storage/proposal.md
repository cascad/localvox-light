# Proposal

## Why

Владелец выбрал единый пользовательский каталог `~/.localvox/models` (на Windows
`%USERPROFILE%\.localvox\models`) вместо привязки моделей к текущей папке установки.
Сейчас downloader, CLI Vosk и остальные загрузчики разрешают пути по-разному.

## What Changes

- Ввести общий resolver моделей и данные расположения в model manifest.
  Явные CLI/env пути имеют приоритет; существующие portable/CWD модели
  переиспользуются; для новой установки выбран пользовательский каталог.
- Согласовать daemon/worker, doctor, setup и downloader/installer, предоставив
  read-only JSON resolved paths для установленного комплекта.
- Сохранить явный `fetch-models --root/-Root` как portable override. Не выполнять
  миграцию, повторную загрузку готовых моделей и перенос хранилища Ollama.
- Показывать фактические пути и документировать новый default. Check не создаёт
  каталогов; загрузка создаёт их только при необходимости, включая Hidden для
  нового стандартного `.localvox` на Windows.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `installation-models`: SETUP-002 дополнен пользовательским fallback;
  SETUP-008 фиксирует общий default, override/legacy reuse и read-only discovery.

## Impact

Core model paths/lang/CLI, doctor, setup wizard, manifest, Windows/Unix downloaders
и installers, package checks, README/user-guide и evidence. Форматы весов,
выбор LLM и реальная пользовательская установка не меняются этой работой.
