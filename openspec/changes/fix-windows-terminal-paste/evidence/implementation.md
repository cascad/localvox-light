# Ввод терминала · 2026-11-11

## Воспроизведение и решение

Исходный native ConPTY probe crossterm 0.28.1 воспроизвёл чтение содержимого
ограниченной вставки как отдельных клавиш: `q` устанавливает quit. Готовый
WindowsTerminal termwiz 0.23.3 дал одну Paste, но потерял `🦀` из UTF-16 surrogate
pair. Ни один из этих вариантов не принят в качестве готового reader.

Production адаптер включает стандартный Windows VT input, читает bounded batches
ReadConsoleInputW, сохраняет UTF-16 между чтениями и передаёт scalars публичному
termwiz InputParser. Codepage, clipboard и настройки терминалов не меняются.
Unix сохраняет crossterm poll/read; имя эмулятора нигде не выбирает поведение.
Документированная OS-граница:
[Microsoft VT input](https://learn.microsoft.com/en-us/windows/console/setconsolemode).

Вставка попадает в App::paste. Ссылка вне ввода открывает форму; только отдельный
Enter создаёт Task::Ingest, Esc не создаёт ничего. Поиск и редактор имеют приоритет.
Ctrl+V, если его получает приложение, читает clipboard только по этому хоткею;
протокольная вставка clipboard не читает. Ошибка чтения сохраняет введённое и
попадает в console. Реальный clipboard владельца в тестах не читался.

## test-run

`cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`:

- 162 library passed / 3 fixture helpers ignored, без исключения paste probe.
- 2 IO, 4 workspace ConPTY и 2 run-script passed; hardware mpv ignored.
- Unit: незавершённая вставка и split CSI/paste delimiters с idle между чтениями,
  RU/EN, supplementary Unicode, control/F10 внутри payload, последующие клавиши,
  arrows/Page/Home/End, F6/F8/F9/F10, Ctrl/Alt/Shift, resize и native repeat.
- Настоящий ConPTY с production reader: begin/end маркеры разделены чтениями с
  80 ms паузами, UTF-8 на PTY краю приходит по байтам. Одна Paste сохраняет всю
  ссылку/emoji, не выставляет quit/palette и не создаёт effects; последующий F10
  распознан, исходные console flags и raw mode восстановлены.
- Error fixture также создаёт production Input и возвращает ошибку: guards
  восстанавливают terminal screen/paste/raw mode. Клиент не закрывает демон.

Release TUI собран успешно в обычный `target/release/localvox-tui.exe`, `--version`
проверен. TEMP/TMP и артефакты проверок находятся на D: внутри target/tmp.

## Граница проверки

Автоматический native Windows ConPTY — test-run. Реальные GUI hosts, macOS/Linux,
SSH/tmux и аппаратный звук — not-run, см. [короткую матрицу](../../../../docs/terminal-compatibility.md).
Одиночный Esc до получения следующего байта имеет стандартную VT неоднозначность;
скорость набора не используется для классификации paste. Терминал без границ
вставки не позволяет отличить injected текст от клавиатуры: сначала открыть поле
через `i`. Этот факт не маскируется обещанием универсального распознавания.
ARCH-014 остаётся delta; main specs не синхронизировались и change не архивирован.
