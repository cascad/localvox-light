# Подтверждение без ввода · 2026-11-12

RUN-006 уточнён отдельной delta по прямому решению владельца. Основание —
add-graceful-application-exit; его delta ещё не в main specs, поэтому sync этого
MODIFIED выполняется после исходного ADDED. Strict change validation проходит,
CLI выдаёт информационное предупреждение о таком порядке archive. Историческое
подтверждение с ДА в предыдущем evidence не переписывалось.

Реализован отдельный FormKind::Shutdown без полей. Ctrl+S/Ctrl+Enter отправляют
прежний Task::Shutdown, Esc отменяет. Десятистрочный диалог сохраняет адрес,
предупреждение и цветную подсказку; редактор/вставка не обращаются к отсутствующему
полю. Остальные typed confirmations и backend lifecycle/API сохранены.

`cargo test --release --locked --jobs 2 -p localvox-light-terminal`,
`CARGO_TARGET_DIR=target/modular-build`, `RUST_TEST_THREADS=2`, `INSTA_UPDATE=no`:

- 212 lib passed / 3 fixture-only ignored: Ctrl+S/Ctrl+Enter без текста,
  повторная отправка, Esc, Tab/BackTab/Enter/буквы/paste, pending/error restoration.
- 8 PTY tests passed: настоящий Windows ConPTY подтверждает full exit одним
  Ctrl+S без вставки ДА; прежние paste/resize/reconnect/client-only exit сохранены.
- 2 IO и 4 launcher/CLI passed. Один media test explicitly ignored.
- Render assertions 80/120: адрес, предупреждение, клавиши; поля ввода нет.
  Snapshot baselines не менялись. TUI/vox release executables собраны и проверены
  CLI/ConPTY tests; приложение владельца не запускалось.

Лог: `target/tmp/simple-exit-tests.log`. User guide US-062, README и terminal guide
обновлены. Внутренний статус Mac приёмки убран из README по просьбе владельца;
not-run в предыдущем evidence сохранён. Реальные audio/Mac/hosted проверки not-run.
