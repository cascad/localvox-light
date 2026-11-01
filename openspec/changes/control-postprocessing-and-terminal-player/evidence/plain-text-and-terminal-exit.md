# Цельный текст и выход из терминала · 2026-11-01

Уточнение после просмотра TUI владельцем: без ручной очистки вкладка «Текст»
показывает всю первичную расшифровку единым документом. Также владелец сообщил,
что Ctrl+Q не закрывает клиент в терминале VS Code.

Этот прогон дополняет [предыдущий](transcript-summary-timeline.md).
Пользовательские архив, .env, демон и настройки VS Code не изменялись.

## Поведение и свидетельства

| Требование | Реализация | Проверка |
|---|---|---|
| ARCH-007: цельный исходный текст | [UI](../../../../ui/src/DocumentPane.tsx), [проекция TUI](../../../../crates/localvox-light-terminal/src/render/documents.rs), [копирование/экспорт](../../../../crates/localvox-light-terminal/src/app/documents.rs) | Фрагменты соединены пробелами, без добавленных диалоговых тире, имён и таймкодов. Raw Text — один документ без выделения отдельной реплики; переносы/прокрутка проверяются на 120×36 и 64×24. Обычные processed snapshots остаются отдельными. |
| ARCH-007: одно копирование целиком | [клавиши TUI](../../../../crates/localvox-light-terminal/src/app.rs), [unit](../../../../crates/localvox-light-terminal/src/app/tests.rs), [browser e2e](../../../../ui/tests/e2e/workspace.spec.ts) | y/н в TUI копирует все 2000 фрагментов независимо от item/scroll. Ctrl+Shift+C и кнопка UI копируют все 500 фрагментов, включая подтверждённый через DOM Range текст вне экрана. Формы сохраняют буквенный ввод; shortcut UI не забирает ввод редактора. Служебная подпись не копируется. |
| TERM-007: альтернативы выхода | [обработчик](../../../../crates/localvox-light-terminal/src/app.rs), [ConPTY](../../../../crates/localvox-light-terminal/tests/pty_workspace.rs), [подсказки](../../../../crates/localvox-light-terminal/src/render.rs) | q/й вне формы, F10 также внутри формы; Ctrl+Q сохранён. Unit проверяет отсутствие stop/других эффектов. Native terminal проверяет F10 из формы и q после reconnect, восстановление alternate screen/bracketed paste и сохранение серверной записи. |

Слова, имена и таймкоды сохраняются в API и вкладке «Реплики». Изменена проекция
raw fallback, а не первичная расшифровка или сохранённая ручная очистка. Обработчик
фрагмента TUI для цельного raw Text использует весь интервал документа, не скрытый
индекс прежней реплики. Модель для отображения не вызывается, fake cleanup/file
не создаются; проверки resolver/API перечислены в предыдущем evidence, повторный
прогон моделей здесь не выполнялся.

## Команды

Cargo: `--locked -j 2`. Сначала клиент собирался в `target/tui-preview`, поскольку
старый пользовательский localvox-tui.exe удерживал основной exe. К повторной
проверке старый процесс уже завершился; агент не останавливал демон/worker.
Затем выполнен весь пакет проверок и обновлён обычный `target/debug/localvox-tui.exe`.
Локальные логи: `target/plain-text-*`; они не являются версионированным evidence.

| Проверка | Результат |
|---|---|
| `cargo test --target-dir target/tui-preview -p localvox-light-terminal -- --test-threads=2` | 93 passed, 3 ignored: 2 helper вызываются родителями, реальный media backend отдельно ignored; в том числе 87 unit и 2 ConPTY |
| `cargo test -p localvox-light-terminal -- --test-threads=2` | Те же 93 passed, 3 ignored из обычного target/debug; обновлён основной exe |
| `cargo fmt -p localvox-light-terminal -- --check` | PASS |
| `npm --prefix ui test` | 9 passed |
| `npm --prefix ui run test:e2e` | 34 passed, включая 3 ожидаемые отрицательные проверки визуального детектора |
| `npm --prefix ui run build` | PASS: TypeScript/Vite |
| `npm --prefix ui run lint` | PASS, 8 прежних warnings: 6 Fast Refresh и 2 ref cleanup; ошибок нет |
| `cargo test -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | 1 passed: настоящий API, встроенный UI в Chromium и новый TUI в ConPTY; временный архив, без моделей |
| Строгая валидация main specs и двух затронутых changes | PASS: 15 main specs, control-postprocessing-and-terminal-player, add-technical-journal-workspace |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 86 main требований, 168 сценариев, 86 строк coverage |
| `git diff --check` и проверка новых текстовых файлов | PASS; snap.new не осталось |

Перед принятием просмотрены raw-text PNG, два новых symbol/style snapshots цельного
текста и изменения 15 существующих TUI snapshots. В последних меняется только
нижняя подсказка клавиш и её style spans. После принятия baseline выполняется
обычный прогон без автоматического обновления снимков.

Совместный тест сначала также выполнен с preview-клиентом: свежий API test exe
скопирован в preview/debug/deps, чтобы штатный поиск соседнего TUI находил именно
новый клиент. Первый запуск не загрузился без runtime DLL; путь к обычным debug и
debug/deps добавлен только в окружение тестового subprocess, затем 1 passed.
Итоговый прогон выше выполнен обычной командой Cargo с основным клиентом.

## Границы

- В [документации VS Code](https://code.visualstudio.com/docs/terminal/advanced)
  описан перехват сочетаний до передачи в shell. Это возможное объяснение Ctrl+Q,
  а не установленная причина в конфигурации владельца. Её отдельная проверка
  не выполнялась; настройки/keybindings не переписывались.
- Native ConPTY проверяет получение клавиш новым приложением и завершение клиента;
  физический ввод в конкретном окне VS Code остаётся not-run.
- UI e2e использует подменённый clipboard sink, TUI unit — Task::Copy. Физический
  системный буфер отдельно не проверялся. Новый shortcut вызывает существующий
  механизм копирования.
- Физические микрофон/звук и настоящая двухчасовая модельная обработка не запускались;
  пункт 10.2 add-technical-journal-workspace остаётся not-run.
- Main specs не синхронизировались, changes не архивировались. Коммиты пользователь
  выполняет вручную; агент не менял индекс.
