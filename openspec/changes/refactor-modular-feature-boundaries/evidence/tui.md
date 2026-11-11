# Срез 1 · TUI boundaries, 2026-11-11

Контрольная точка b3766b6. Внутренний рефакторинг; US-001–056, команды,
HTTP/CLI, визуальные baselines и форматы данных не менялись.

- UiPreferences объединяет theme/color/ascii/keymap/list width; App.reconnected
  переносит preferences и console целиком, очищая connection data/pending effects.
- App.responses владеет приёмом ответов/epoch/partial acceptance; App.input —
  клавиатурной маршрутизацией. App остаётся детерминированным состоянием/действиями.
- Renderer разделяет lists/playback/overlays и прежние document projections.
- InputSource.next_event отделяет цикл приложения от platform reader. Реальный
  Windows/portable input сохраняется; recorded adapter проверяет тот же Action/
  inert Paste путь без чтения clipboard и запуска операций.

## Test-run

TEMP/TMP на D:, Cargo --jobs2, INSTA_UPDATE=no.

- **198 library passed / 3 helpers ignored**, включая два новых contract tests:
  все preferences сохраняются при reconnect, старые mutations/documents/player
  не переносятся; injected source использует те же resize/palette/paste semantics.
- **5 actual ConPTY**, **2 IO**, **2 run-script passed**, hardware mpv ignored.
  Все прежние snapshots прошли без обновления. Реальная raw-mode restoration,
  bracketed paste, модификаторы, reconnect/keymap/width и growing partial сохранены.
- [Карта](../../../../docs/modularity.md) содержит все 56 уникальных US-ID из
  user-guide, существующие владельцы и test-linked места. Это карта проверки,
  не заявление аппаратной приёмки каждого сценария.

Лог: ignored `target/tmp/modular-tui-boundaries.log`. Первоначальные ошибки
компиляции при переносе приватных функций исправлены до успешного полного прогона.
Реальные устройства/OS bindings/Mac этим срезом не проверялись. Оставшиеся этапы
change не объявляются выполненными этим коммитом.

Release TUI/--version прошли; review/vim-navigation обновлён. SHA256:
`009ce5ffa2ad4c1f553af0a789c27921904547b5d72f6785c53f410269adf48d`. Strict main 15/0, strict change valid, check-openspec PASS
(15/88/188, 1865 links); scoped rustfmt/diff проверены.
