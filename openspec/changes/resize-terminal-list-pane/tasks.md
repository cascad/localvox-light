# Tasks

## 1. Регулировка ширины

- [x] 1.1 Добавить requested/effective geometry, hotkeys/palette/reset/reconnect и journal anchor; unit/Buffer tests проверяют bounds/default/resize/input priority/state/audio/anchor, обновить help и US-056.
- [x] 1.2 Проверить Control arrows и сохранение ширины на actual ConPTY/reconnect, пройти terminal suite с INSTA_UPDATE=no и убедиться, что прежние baselines не меняются.

## 2. Поставка

- [x] 2.1 Strict main/change/check-openspec/diff, scoped rustfmt; release TUI и review exe, evidence/coverage и scoped commit. Внешние bindings ОС/terminal и ручная эстетика not-run.
