# Полный выход · 2026-11-12

RUN-006 реализован в delta; main specs не sync/archive. Обычный выход не меняет
daemon lifecycle. Полный — отдельная подтверждаемая команда F2.

## Реализация

- API application control разделяет running с прежним Daemon. Shutdown request
  не выполняет cleanup в HTTP: его выполняет owner в обычном bounded shutdown.
  После сбора engine/autocook errors owner публикует результат и ждёт ответный
  guard до 2 секунд. На signal/tray без HTTP запроса дополнительного ожидания нет.
- POST /api/application/shutdown проходит auth/origin/body budget, требует
  confirm=true; сервер без owner отвечает 501, повтор — 409. Во время остановки
  новые mutations получают 503. HTTP ожидание — до 60 секунд; клиентский POST —
  до 75 секунд, без retry/bootstrap. Успех требует completed=true.
- TUI показывает адрес, предупреждение, ДА и клавиши подтверждения; ожидание имеет
  таймер. Новые действия/смена подключения блокируются, periodic polling заморожен.
  Уже пришедшие ответы освобождают bookkeeping. Ошибка попадает в общую консоль,
  форма сохраняется; только подтверждённый успех закрывает клиент.

## Выполнено

- Aggregate release run четырёх crates: app 16 lib passed / 11 explicit ignored,
  2 main passed (включая настоящий Daemon::shutdown с isolated component threads),
  10 setup passed; launcher hardware/process fixture explicitly ignored.
- API 91 lib passed / 8 explicit ignored, 3 integration passed. Новые control/HTTP
  tests проверяют result-before-reply/drain, failure, timeout/abandoned guard,
  auth/origin/confirmation/body limit, unavailable runtime, duplicate и новые mutations.
  Engine/HTTP fixtures пишут только временные файлы, реальный захват не запускается.
- Client 3 lib + 10 transport passed: endpoint/body/auth, только completed=true,
  потерянный acknowledgement без второго POST.
- TUI 211 lib passed / 3 explicit fixture-only ignored; 2 IO passed. Новая форма
  проверена render assertions при ширине 80/120. Existing snapshot baselines сохранены.
- После исправления нового native test все 7 ConPTY passed последовательно:
  vox portable config/reuse, обычный выход с продолжающейся записью и отдельный
  подтверждённый full exit с ожиданием ответа/восстановлением terminal modes.

Первый aggregate run **не объявляется успешным целиком**: ConPTY завершился 5/7.
Новый тест ошибочно отправлял Esc и `:` одним пакетом (интерпретировалось как Alt);
исправлен ожиданием видимого закрытия формы перед открытием палитры. Прежний
resize/compact-footer test также падал при parallel run; sequential repeat прошёл.
Дополнительная диагностика установила причину: конкатенация Cell::contents удаляла
не записанные blank cells, превращая видимое «F8 stop» в «F8stop». Проверки регионов
переведены на Screen::contents_between с теми же координатами/ожиданиями; добавлен
детерминированный regression case с cursor skip. Product rendering не менялся.
Финальный repeat с `RUST_TEST_THREADS=2` прошёл полностью: 211 TUI lib,
2 IO, 8 PTY tests (7 настоящих ConPTY и 1 blank-cell regression), 4 launcher;
client 3 lib / 10 transport. Прежние 3 fixture-only и 1 media ignored сохранены.
Ни координаты footer/page assertions, ни snapshot baselines не ослаблялись.

Локальные логи: `target/tmp/exit-full-tests.log`, `exit-pty-final.log`,
`exit-tui-final-all.log` (диагностика blank cells),
`exit-tui-final-verified.log` (окончательный pass).
Неподтверждённые/первые failed результаты сохранены.

Финальная release build daemon/TUI/vox passed. Пересобран полный Windows zip
`target/setup-delivery-check/localvox-x86_64-pc-windows-msvc.zip`; package startup
probes passed. Strict main specs и оба change, check-openspec, scoped rustfmt,
actionlint и diff passed. Отдельные логи: `exit-final-build.log`,
`exit-final-package.log`, `exit-main-specs.log` в target/tmp.

## Границы

Реальный microphone/system capture, Mac/Linux и hosted jobs нового commit — not-run.
Daemon владельца не запускался/не останавливался, пользовательские архив/.env
не изменялись. Аппаратное сохранение последнего аудиофрагмента нужно проверить
владельцу; tests доказывают wiring/порядок/result, не заменяют эту приёмку.
