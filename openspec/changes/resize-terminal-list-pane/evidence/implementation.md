# ARCH-019 · Ширина списков, 2026-11-11

Реализованы Ctrl+Left/Right в menu/list (4 ячейки), три действия палитры
«Список: уже / шире / стандартная ширина» и default27. Общая geometry оставляет
списку ≥22, документу ≥40 и видимому context28. Compact <95 сохраняет прежнюю
однопанельную схему и сообщает ограничение; reset явно возвращает preference27.
Requested width сохраняется между разделами и при reconnect; resize окна только
ограничивает effective width. Сохранения на диск нет.

Формы, console, help, palette и ввод фильтра получают приоритет. Control arrows
работают как resize только в menu/list; player arrows и Alt seek сохранены.
Ширина меняется без API/player effects, смены выбора/документа/фильтра/epoch.
Ручная позиция журнала использует существующий anchor при изменении переносов.
Help и US-056 обновлены. UI/API/pipeline/модели не менялись.

## Test-run

TEMP/TMP на D:, Cargo `--jobs 2`, `INSTA_UPDATE=no`.

- **196 library passed / 3 helpers ignored**: новые проверки bounds всех panes,
  default, temporary window clamp и восстановления preference; шаг от visible width,
  reset/compact, input priority, repeat/release, menu/list, сохранение data/audio/effects.
- Actual Buffer в четырёх темах: граница документа переходит с x27 на x31 и обратно;
  измеряемая ширина текста меняется согласованно. Manual journal anchor сохраняет
  operation/relative offset при изменении числа wrapped rows. Прежние snapshots
  прошли без обновления baselines.
- **5 actual ConPTY passed**. Существующий paste/resize/restore сценарий теперь
  отправляет CSI Control+Right/Left и проверяет реальную границу ASCII панели;
  Stop не вызывается. Reconnect сценарий расширяет список до31 до подключения,
  затем проверяет сохранение границы после нового App и повторной загрузки данных.
- **2 IO / 2 run-script passed**, hardware mpv ignored. Ошибки/фильтры/quit/clipboard/
  текущие visual и paging сценарии проходят в полном terminal package.

Команда: `cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`.
Полный лог — ignored `target/tmp/pane-width-terminal.log`; ранний unit прогон —
`pane-width-unit.log`. Работающий daemon и личный архив не запускались.

ARCH-019 остаётся delta, main specs не синхронизированы. Отображение в терминалах
владельца/Mac и внешние OS/terminal bindings **not-run**; палитра остаётся доступной,
если ОС перехватывает Control arrows. Это не обещание обхода внешнего перехвата.

Release TUI собран и `--version` прошёл; обычный и review/vim-navigation exe
обновлены, SHA256: `56a04a280256dd5000bcf6fc94e08032ea67b83db6fe72dfd91f53ca96292852`. Strict main: 15 passed / 0 failed;
strict change valid; check-openspec PASS (15/88/188, 1862 links). Scoped rustfmt
и diff проверены. Посторонние изменения/секреты/target outputs не коммитятся.
