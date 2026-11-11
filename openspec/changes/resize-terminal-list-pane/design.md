# Design

## Context

См. proposal. `workspace_columns` используется и renderer, и измерениями страниц.
Сейчас там фиксированные 27/Min40/28; <95 — существующий compact layout.
Alt+Left/Right уже перематывают аудио. Reconnect создаёт новый App, сохраняя тему/keymap.

## Goals / Non-Goals

**Goals:** одна геометрия для рендера и навигации, безопасные границы и быстрый сброс.

**Non-Goals:** drag мышью, новый layout, запись в .env/диск, UI и изменение плеера.

## Decisions

- App хранит requested width (default27). Общий helper вычисляет effective width:
  clamp к 22..(window − Min40 − видимый context28). Resize окна не меняет requested.
  Действия начинают шаг от effective, чтобы каждое нажатие сразу было заметно.
- Ctrl+Left/Right только в menu/list, с точным CONTROL modifier; формы/overlays и
  filter editing имеют приоритет. Alt seek и player arrows сохраняются. Палитра
  даёт путь для ОС/терминалов, перехватывающих Control arrows.
- Три Action в конце палитры: уже/шире/стандартная ширина. Новые screen/geometry
  и реальные ConPTY assertions проверяют действие, не вводят emulator-specific logic.
- Использовать существующий journal anchor при явном изменении ширины. Выбор,
  playback/effects/epoch не меняются. Requested width переносится при reconnect.
- Preference живёт только в текущем клиенте, как переключение темы; новый запуск
  начинает с хорошего default27. Компактное окно сообщает ограничение; reset
  явно возвращает preference27 и доступен также через палитру в компактном режиме.

## Risks / Trade-offs

- Перехват ОС/terminal → palette actions, без обещания обхода внешних bindings.
- Расхождение pager/render → shared geometry и actual Buffer tests на границах.
- Сдвиг журнала при переносах → прежний anchor, regression с длинными строками.
- Неожиданная правка поля/seek/stop → priority/modifier/state/effect tests и ConPTY.

## Migration Plan

Пересобрать только TUI, обновить обычный/review exe. Daemon/архив/модели не запускать.
