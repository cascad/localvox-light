# Командные клавиши · 2026-11-11

ARCH-017 реализован: UI copy использует KeyC + modifiers с editor/IME/repeat guards.
TUI --keymap / LOCALVOX_TUI_KEYMAP загружает строгий version=1 JSON до подключения
и terminal mode. Custom command projection предшествует RU/EN; исходные события
сохраняются для текстового ввода. Единый resolver охватывает qq/Ctrl+Q, Slash/Ctrl+U,
form control-команды, console/palette и runtime Ctrl+V; reconnect сохраняет keymap.
Штатная раскладка/данные/clipboard backend не изменялись.

## Test-run

TEMP/TMP на D:, cargo --jobs 2, INSTA_UPDATE=no.

- Terminal package: **189 lib passed**, 3 прежних helpers ignored; **2 IO / 2 run-script
  passed**, hardware mpv ignored. Снимки прошли без изменения baselines.
- Actual ConPTY: **5 passed**; custom λ открывает команду перед reconnect, ωω закрывает
  после него; исходный фильтр/паузы/paste/restore сохранены. Invalid/missing keymap
  завершаются до terminal escape sequences и подключения. Unit покрывают bounded file,
  invalid schema/версии/Unicode/targets, overrides/case/Control, qq, форму и фильтр;
  Ctrl+V читает stub clipboard, protocol paste его не читает.
- UI build и **14 unit passed**; **2 browser e2e passed** (обычный raw Text/offscreen
  screenshot/editor и physical KeyC с русским/греческим/китайским/иным key, Meta,
  IME/repeat/Alt/другая code). Clipboard подставной, браузер настоящий.
- Release TUI build и --version прошли; обычный и review/vim-navigation exe обновлены.
  SHA256: f78a50a4b9ba9251bef51fc59815e19682b5caf362c4e0d269a7ccbc6c52bd72.
- Strict main: 15 passed; strict change valid; check-openspec PASS (15/88/188), diff clean.
  UI dist собран; поставка встроенного UI в daemon проверяется с итоговым комплектом.

Документация: US-012/054, terminal-workspace и рабочий example JSON. Main specs не
меняются, ARCH-017 остаётся delta. Реальные раскладки/IME на устройствах владельца,
Mac и все terminal hosts **not-run**. Поглощённую оболочкой клавишу конфиг не восстановит.
