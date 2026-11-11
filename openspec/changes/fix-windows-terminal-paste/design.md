# Design

## Context

См. proposal.md. Native ConPTY воспроизводит дефект crossterm 0.28.1: без
ENABLE_VIRTUAL_TERMINAL_INPUT host фильтрует маркеры, а pasted `q` вызывает выход.
Обновление до 0.29 не меняет этот backend. Termwiz 0.23.3 выделяет Paste, но его
WindowsTerminal.decode_input_records теряет UTF-16 surrogate pairs; готовый reader
не подходит. Использовать идентификатор WezTerm, скорость набора или чтение
clipboard без явного действия запрещено выбранным решением.

## Goals / Non-Goals

**Goals:** безопасный paste/key контракт в основных терминалах на всех трёх ОС;
проверяемая OS-граница Windows, Unicode и восстановление режимов.

**Non-Goals:** изменение конфигурации эмуляторов, переустройство renderer/HTTP,
объявление не запущенных терминалов прошедшими аппаратную приёмку.

## Decisions

- Вынести чтение в небольшой адаптер Input, возвращающий прежние crossterm Event.
  Unix сохраняет существующий poll/read. Windows использует VT input, bounded
  ReadConsoleInputW и WaitForSingleObject, без фонового потока и его отмены.
- Windows-адаптер преобразует последовательность UTF-16 key-down code units в
  Unicode scalars, сохраняя high surrogate между чтениями. ANSI/VT последовательности
  разбирает публичный termwiz InputParser; готовый WindowsTerminal не используется.
  Это стандартный OS/протокольный мост, без копии escape parser или vendor fork.
- Не определять вставку таймингом или строкой URL. Границы — bracketed paste;
  незавершённая вставка не передаётся App. Обычная неоднозначность одиночного Esc
  разрешается на idle poll; уже начатый многобайтовый prefix не сбрасывается.
- Существующие редакторы/поиск получают App::paste. Вне ввода одиночный HTTP(S) URL
  открывает заполненный Import; отправка только отдельным Enter. Другая вставка
  не превращается в команды. Сетевое API остаётся без изменений.
- Если терминал отдаёт Ctrl+V приложению вместо выполнения собственной вставки,
  runtime читает clipboard ровно по этому явному хоткею и вызывает тот же paste.
  Протокольная вставка clipboard не читает; ошибка сохраняет черновик и идёт в console.
- Mode guard восстанавливает исходные Windows flags до внешнего TerminalGuard;
  console codepage и clipboard не меняются. Ошибка не переключает обратно на
  заведомо теряющий границы reader.

## Risks / Trade-offs

- Дополнительная Windows-only зависимость на parser termwiz → pinned release,
  протокольные unit-тесты и реальные ConPTY с production adapter.
- Отличия key encoding/модификаторов → тесты arrows/Page/Home/End, F keys,
  Ctrl/Alt/Shift, RU/EN, фрагментации и Unicode; ручная матрица отдельно.
- Терминал без протокольных границ не позволяет отличить injected текст от
  физического набора → не обещать невозможную универсальность, дать явный вход
  через `i` перед вставкой; проверить фактическое поведение старых hosts отдельно.

## Migration Plan

Обновить TUI после тестов. HTTP, архив и настройки не мигрируют; rollback — прежний
бинарник. Реальные Windows Terminal / VS Code / WezTerm, macOS Terminal / iTerm2 и
Linux terminal / SSH / tmux остаются not-run до прогона, независимо от unit/PTY.
