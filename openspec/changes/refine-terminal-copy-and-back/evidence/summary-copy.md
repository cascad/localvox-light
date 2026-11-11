# Выбранный блок сводки · 2026-11-11

Дополнение ARCH-016 по прямому уточнению владельца после `316a6f2`.
Во вкладке «Сводка» y/н теперь копирует только выбранный SummaryBlock.text.
Новая команда «Копировать выбранный блок сводки» использует тот же Task::Copy;
Ctrl+Y/CopyDocument/экспорт сохраняют полную сводку. Явный выбор по tab/item,
без разбора отрисованных строк; пустой/отсутствующий блок не заменяет clipboard
всем документом. Реплики/Текст/редакторы и иерархия Esc сохранены.

Код: [dispatch](../../../../crates/localvox-light-terminal/src/app.rs),
[projection](../../../../crates/localvox-light-terminal/src/app/documents.rs),
[actions](../../../../crates/localvox-light-terminal/src/app/commands.rs).
Обновлены help/footer/context и сценарий US-053 в пользовательском справочнике.

## Test-run

`TEMP/TMP=D:\rust_repo\localvox-light\target\tmp`, Windows, cargo `--jobs 2`.

- `INSTA_UPDATE=no cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`:
  **185 lib passed**, 3 прежних helpers ignored; **2 IO / 4 workspace ConPTY /
  2 run-script passed**. Аппаратный mpv test ignored.
- Новые [проверки Summary](../../../../crates/localvox-light-terminal/src/app/navigation/tests.rs):
  выбранный второй блок для heading/bullet/paragraph, multiline/Unicode, источники и
  provenance не копируются, item после Down, RU/EN, палитра, полный Ctrl+Y/CopyDocument,
  пустой/out-of-range/отсутствующий блок, неподходящая вкладка, отказ clipboard с
  сохранением текста/выбора, буквальный ввод y/н в форме. Прежние проверки Реплик,
  полного «Текста», Esc и late replies также прошли.
- **12 snapshots / 51 строка просмотрены**: изменения ограничены подсказкой копирования
  в footer и правым context сводки в широком окне. Bounds/число строк, текст документов
  и стили основной области прежние; повторный прогон с INSTA_UPDATE=no успешен.
- Main specs strict: **15 passed / 0 failed**; change strict valid. Прежняя INFO о
  зависимости ARCH-011 от `add-terminal-session-activity` сохраняется; sync/archive
  не выполнялись. `python scripts/check-openspec.py` PASS: 15/88/188; diff проверен.

Release build (`--locked --release -p localvox-light-terminal --bin localvox-tui --jobs 2`)
успешен. Обычный `target/release/localvox-tui.exe` и прежний review/vim-navigation
обновлены, оба `--version` успешны, процессы не завершались.
SHA256: `b06ffc12da773e15781864d8f32e3b800aefa9b90d03ea6ab1eb4986e0c033ea`.

Ручной clipboard в терминале владельца остаётся **not-run**, см. обновлённый M16.
Тесты проверяют payload общего clipboard backend без изменения настоящего буфера.
Демон, модели и пользовательский архив не запускались/не изменялись.
