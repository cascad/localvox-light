# Design

## Context

См. proposal. В браузере доступен KeyboardEvent.code. Терминальный KeyEvent хранит
символ/модификаторы; current RU/EN mapping уже применяется после обработки ввода.
Quit, локальный filter и Ctrl+V находятся раньше обычных команд и требуют общего
разрешения, иначе keymap срабатывал бы только в части интерфейса.

## Goals / Non-Goals

**Goals:** единая нормализация команд с сохранением Unicode ввода и paste.

**Non-Goals:** распознавание раскладки по тексту, глобальные keyboard hooks, изменение
терминала пользователя, поддержка нового keyboard protocol, перестановка панелей.

## Decisions

- UI буквенные сочетания сверяют code (KeyC), modifiers/repeat/isComposing и editable
  target. Named Enter/Space/arrows уже независимы от раскладки.
- JSON version=1, keys — отображение одного Unicode scalar в печатную ASCII команду.
  Верхний регистр наследует shifted ASCII позицию; явное соответствие имеет приоритет.
  Не более 128 соответствий/64 KiB, строгая схема. CLI/env только явно выбирают файл;
  автопоиска по cwd нет. Ошибка завершается до terminal raw mode/daemon startup.
- App.shortcut_key даёт командную проекцию отдельно от исходного key. Она применяется
  один раз; custom override предшествует RU/EN. Form/filter получают исходные буквы;
  control-команды, qq, Slash trigger, palette/console и runtime Ctrl+V видят проекцию.
  Reconnect переносит keymap как тему, не сбрасывая настройку.
- Меняется перевод клавиш в штатные команды, не таблица действий/таймер qq/clipboard.
  Это сохраняет существующие контексты без второго dispatcher. F10/стрелки доступны
  независимо от буквенной раскладки. Example config и явные ограничения документированы.

## Risks / Trade-offs

- Перехват печати/IME → отрицательные e2e/unit, исходный key не заменяется в редакторе.
- Пропуск ранних shortcut branches → единый resolver и tests Ctrl+V/Ctrl+Q/qq/Slash/
  Ctrl+U/Ctrl+S/поиск/формы/reconnect. Invalid config/версии/длины проверяются до UI.
- Терминал поглотил клавишу → конфиг не восстановит недоставленное событие; named
  navigation/exit остаются запасными, ограничение явно указано.
