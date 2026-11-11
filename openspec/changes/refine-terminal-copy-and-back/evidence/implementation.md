# Контекстное копирование и Esc · 2026-11-11

## Поведение и границы

ARCH-016 реализован в [App/navigation](../../../../crates/localvox-light-terminal/src/app/navigation.rs),
[actions](../../../../crates/localvox-light-terminal/src/app/commands.rs) и
[copy projection](../../../../crates/localvox-light-terminal/src/app/documents.rs).
y/н в «Репликах» копирует только выбранный text, без speaker/time/original;
Ctrl+Y/«Копировать документ» и экспорт сохраняют полную projection. В «Тексте» y
копирует всё. Пустой/отсутствующий выбор не заменяет clipboard другим документом.
Применяется прежний Task::Copy; успех теперь нейтрально подписан «Скопировано в
буфер обмена», отказ остаётся в console и не стирает текст/выбор.

Esc закрывает overlay, затем player→details→list→menu, на корне инертен. Notes
Enter открывает детали выбранного слота. Для записи из Queue/Search хранится один
типизированный родитель: session задания или смысловой ключ hit и offset. При
возврате выбор ищется заново, исчезнувший элемент даёт безопасный первый выбор,
offset ограничен текущим размером окна. Back не вызывает select_session/Stop/Load;
смена раздела и явный select_session отменяют старый parent. Отменённый Search intent
не повторяется; его ещё не отправленные effects удаляются, поздние ответы отсекает
epoch. Нет стека истории, таймеров или привязки к конкретному эмулятору.

Уточнение ARCH-011: Esc при вводе фильтра по-прежнему очищает и завершает ввод.
Вне ввода Esc уводит в меню, сохраняя фильтр; Slash→Esc явно очищает его. Актуальные
delta/design `add-terminal-session-activity`, подсказки и справочник US-051/052
согласованы. Старые свидетельства не переписаны.

## Test-run

Windows, `TEMP/TMP=D:\rust_repo\localvox-light\target\tmp`, cargo `--jobs 2`.

`INSTA_UPDATE=no cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`:

- **183 lib passed**, 3 прежних fixture/probe helper ignored.
- **2 IO passed**, **4 workspace ConPTY passed**, **2 run-script passed**.
- Hardware mpv test ignored; реальный clipboard и аудиовыход владельца не трогались.
- Новые [9 unit tests](../../../../crates/localvox-light-terminal/src/app/navigation/tests.rs):
  RU/EN y, выбранная вторая/Unicode/многострочная реплика и полный Ctrl+Y/палитра;
  пустой/out-of-range выбор, ошибка clipboard; буквальный ввод; все 7 View и overlay
  priority; Notes Enter; saved filter; Queue/Search parent, reorder/remove, valid
  page/clamping, отмена parent явным выбором/разделом, late Search/document responses,
  отсутствие player effects и сохранение позиции/паузы. Existing partial test также
  проверяет выбранный text после дополнения подтверждённого префикса.
- В actual ConPTY проверены детали→список→меню, сохранённый фильтр, возврат/повторное
  открытие, явный сброс Slash→Esc и прежнее листание/Unicode/paste/quit/terminal restore.
- **37 snapshots просмотрены**: bounds/число строк прежние; изменения текста только
  в footer (34) и подписи сброса фильтра на нижней границе списка (3). Снимки тем,
  ASCII/no-color/16-colors и размеров повторно прошли с `INSTA_UPDATE=no`.

Первый полный прогон выявил magic palette index в reconnect ConPTY: добавленная
команда сдвинула номер «Подключение». Тест теперь находит Action::Connection через
COMMANDS и по-прежнему проверяет открытую форму/секрет/отсутствие повтора Stop.
Окончательный полный прогон прошёл. Два прежних `unused_unsafe` warnings в Windows
input tests не относятся к этому изменению.

## Документация

- `openspec validate --specs --strict --no-interactive`: **15 passed / 0 failed**.
- Strict `refine-terminal-copy-and-back` и `add-terminal-session-activity`: valid.
- CLI отдельно сообщает, что standalone archive MODIFIED ARCH-011 пока невозможен:
  требование добавляет ещё не перенесённая delta `add-terminal-session-activity`.
  Зависимость записана в proposal; sync/archive здесь не выполнялся.
- `python scripts/check-openspec.py`: PASS, 15 capabilities / 88 requirements /
  188 scenarios; main specs не менялись. Diff проверен.

## Ручная приёмка

Release `cargo build --locked --release -p localvox-light-terminal --bin localvox-tui
--jobs 2` прошёл. Обычный `target/release/localvox-tui.exe` и прежний
`target/review/vim-navigation/localvox-tui.exe` обновлены; оба `--version` успешны.
SHA256: `5b2b7126ede72901e6faac9a66d35d65b646d6d33a7ebaf3265b2647a735926b`.
Работающий демон/архив не затрагивались, процессы не завершались.

[M16](../../../../docs/manual-acceptance.md) остаётся **not-run**: проверить clipboard
в своём терминале, выбранную реплику/весь текст, Notes и возврат из Search/Queue при
реальном воспроизведении. Проверки не подтверждают поддержку всех реальных terminal
hosts или аппаратный звук. Общая приёмка раннего распознавания/слабого Mac остаётся
открытой; defaults/пайплайн/UI/архив не изменены.
