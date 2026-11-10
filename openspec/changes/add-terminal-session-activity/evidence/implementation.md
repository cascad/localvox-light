# TUI activity/filter/keys · 2026-11-10

Реализовано по двум замечаниям владельца. Только TUI: server/API/scheduler/model
не менялись. Открытый рабочий демон не останавливался и пользовательский архив
не использовался для проверки. Предыдущий incremental reference/default gate
остаётся открытым; это изменение его не пересматривает.

## Поведение и код

- [Activity](../../../../crates/localvox-light-terminal/src/render/activity.rs):
  braille snake/ASCII spinner только для сообщённого running, static queue/waiting/
  retry/cancel/error/ready, отдельная подпись записи, известная matching phase/position.
  Semantic colors сохраняются на выбранной строке; ASCII/monochrome читаемы.
- [Runtime](../../../../crates/localvox-light-terminal/src/runtime.rs): 100 ms кадр
  в существующем event loop, только если список показан и содержит видимый running.
  Новых потоков/poll запросов нет. Скрытый фильтром running не требует анимации.
- [Фильтр и клавиши](../../../../crates/localvox-light-terminal/src/app/archive_list.rs),
  [App](../../../../crates/localvox-light-terminal/src/app.rs): полный sessions хранится
  отдельно, фильтр Unicode substring без учёта регистра, visible selection по ID.
  Набор/вставка не переключают документ/звук; Enter открывает выбранную видимую запись.
  Ошибка глобального poll сохраняется при смене документа/раздела до свежего ответа.
- Командные соответствия RU/EN применяются после текстовых контекстов: g/п,
  Slash/точка, остальные буквы и Shift. Crossterm даёт символ, не универсальный
  physical scancode; произвольные раскладки не заявлены. Фильтр/формы не транслитерируются.
  В списке буквы, цифры и пробел — ввод; F2/двоеточие открывают действия,
  F8/F9/F10/Ctrl+Q сохраняют глобальную функцию. F12 открывает ошибки и из фильтра.
- [US-006/011/015/024/041/048](../../../../docs/user-guide.md) и
  [руководство](../../../../docs/terminal-workspace.md) обновлены вместе с подсказками.

## Фактически выполнено

TEMP/TMP на D:, `RUST_TEST_THREADS=2`, locked/offline, `INSTA_UPDATE=no` в финальном прогоне:

```powershell
cargo test --locked --offline -j2 -p localvox-light-terminal -p localvox-light-client -p localvox-light-process
cargo build --locked --offline --release -j2 -p localvox-light-terminal --bin localvox-tui
cargo fmt -p localvox-light-terminal -- --check
```

- TUI library **140 passed / 2 ignored**, 10.25 s. В том числе матрица состояний,
  mismatch/stale queue, отказ/восстановление poll и навигация во время отказа,
  неизменная геометрия (между кадрами отличается ровно одна ячейка), выбор/scroll,
  Unicode/casefold/вставка/no-match/refresh/Enter, RU/EN shortcuts и literal form text.
- **4 настоящих ConPTY tests passed**, 12.77 s: действительный клиент/temporary HTTP,
  несколько кадров running без учащения requests, pending/done/failed/unavailable/
  waiting_input, фильтр и открытие верной записи, русские команды времени/поиска,
  ввод исходных символов в форму, paste/resize/выход/восстановление терминала,
  growing partial и masked authorization/reconnect без повторного Stop.
- TUI IO integration **2 passed**; run-script/clean-bundle **2 passed**.
  Client **3 + 8 passed**, process **12 passed / 1 ignored**. Настоящий mpv media
  test в этой серии ignored; звуковой backend не менялся, новый hardware run не заявлен.
- Новые visual baselines: dark/light/16-color/monochrome/ASCII и filtered/empty.
  Просмотрены текстовые buffers/style entries и все 14 изменённых прежних snapshot
  diff: изменения ограничены списком/индикатором/подсказками. Baselines созданы
  явно только при оформлении визуала; финальное сравнение выполнено с обновлением off.
- Проверка обнаружила обрезанную подсказку F8/F10 в узком фильтре — важные клавиши
  перенесены в её начало, прежние проверки всех размеров прошли. Ошибки первых
  ConPTY фикстур (wire key queue, поиск несуществующей экранной подписи) исправлены
  в стенде; ожидания следуют полному экранному состоянию, без фиксированных sleep.
- Release TUI собран, `--version` прошёл. SHA256
  `c801bd2dc96d506120313a014312b356fb36fe8ca180abf7332602e68695f796`.
  Путь: `target/release/localvox-tui.exe`. Для запуска достаточно заменить клиент;
  работающий daemon и worker совместимы, их пересборка/перезапуск не выполнялись.

## Traceability

Новые IDs — delta этого change, не новые принятые main requirements.

| ID | Реализация | Свидетельство |
|---|---|---|
| ARCH-010 | [activity](../../../../crates/localvox-light-terminal/src/render/activity.rs), [render/runtime](../../../../crates/localvox-light-terminal/src/render.rs) | [state/style/frame tests](../../../../crates/localvox-light-terminal/src/render/activity/tests.rs), ConPTY; hardware/нестандартные шрифты not-run |
| ARCH-011 | [archive list](../../../../crates/localvox-light-terminal/src/app/archive_list.rs), [selection](../../../../crates/localvox-light-terminal/src/app.rs) | [filter/input tests](../../../../crates/localvox-light-terminal/src/app/archive_list/tests.rs), filtered snapshots и ConPTY |
| ARCH-012 | [command keys](../../../../crates/localvox-light-terminal/src/app/archive_list.rs) | RU/EN command matrix, literal input, [ConPTY](../../../../crates/localvox-light-terminal/tests/pty_workspace.rs); другие раскладки not-run |

Документационные проверки: strict main **15 passed / 0 failed**, strict change valid,
check-openspec — **88 requirements, 188 scenarios, 88 coverage rows, 1725 links PASS**;
scoped cargo fmt и diff check passed. Windows CI получает strict проверку change
и загрузку новых visual proposals; выполнение удалённого CI не заявляется.
