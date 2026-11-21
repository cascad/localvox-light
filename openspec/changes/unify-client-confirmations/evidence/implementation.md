# RUN-006 / RUN-007 — 2026-11-21

Общий `Form::is_confirmation` используется для Confirm(Task), DiscardSettings
и Shutdown. Полей с ДА нет. Все эти диалоги компактны и показывают действие/цель,
s подтвердить, Esc отменить; Ctrl+S/Ctrl+Enter сохранены.
В настройках первый s открывает подтверждение, следующий s отправляет изменения.
s в текстовом поле остаётся текстом. Действие фиксируется при открытии, API
confirmation/auth/ownership не менялись. UI уже подтверждает кнопками без слов.

Проверено: settings/delete/recook/retry/cancel/shutdown, исходная цель при смене
выбора, pending/error/retry, Esc, Enter, Shift+S, Paste, Repeat, Release,
RU `ы` и отдельный keymap `ω → s`. Один input event не проходит обе стадии.
В legacy VT удержание без Repeat неотличимо от отдельных Press; таймеров и
эвристик для угадывания физических событий нет.

## Проверки

- Полный terminal suite: **220 passed / 3 ignored**. Дополнительно все **8**
  confirmation tests прошли после добавления проверки компактного реального
  TestBackend render (settings/delete/discard на 80×24 без полей/огромного окна).
- Все **9 ConPTY tests passed**. `vox_full_exit_requires_confirmation_and_waits_for_shutdown_ack`
  расширен: настоящее TUI открывает settings, выбирает off стрелками, первый s
  показывает диалог при нуле запросов, второй s отправляет ровно один настоящий
  HTTP POST с ожидаемым set. Затем полный выход тоже подтверждается plain s.
  Расширенный сценарий отдельно прошёл; typed token не посылался.
- UI **83 e2e passed**, в том числе прежние подтверждения кнопками.
  Общий temporary real HTTP → UI + TUI сценарий passed.
- Четыре TUI snapshots изменены ровно `S сохранить → s сохранить`, без иных
  изменений строк/стилей; новый вид диалога проверен render assertions.

TUI/vox в F:\lv обновлены после закрытия владельцем, native probes и bundle-check
прошли. Владелец подтвердил работоспособность. Не выполнялись реальные удаления
пользовательских данных ради тестов.

RUN-006 пока существует в предыдущих принятых change (`add-graceful-application-exit`,
`simplify-terminal-shutdown-confirmation`), но не перенесён в main specs. Строгий
validate сообщает INFO о порядке будущего sync/archive; это не test failure.
Эти изменения не sync/archive автоматически, не переписывают прошлое свидетельство.
