# Постраничная навигация TUI · 2026-11-10

Page Up/Down теперь используют высоту видимого содержимого сфокусированной панели.
Геометрия общая с renderer: [layout helpers](../../../../crates/localvox-light-terminal/src/render/geometry.rs),
[render/measure](../../../../crates/localvox-light-terminal/src/render.rs),
[keys/offset](../../../../crates/localvox-light-terminal/src/app/paging.rs).
Учтены рамка, вкладки/подпись готовой части, двухстрочные записи и нижняя подпись
фильтра. Документ листается по wrapped terminal rows; список перемещает и selection,
и viewport, не открывая запись. Стрелки удерживают viewport; Home/End списка дают
начало/конец. Page при другом фокусе не меняет документ. В журнале ручное чтение
останавливает follow до End. Фильтр/resize/края безопасны, API/плеер не вызываются.

## Выполнено

TEMP/TMP на D:, `RUST_TEST_THREADS=2`, `INSTA_UPDATE=no`:

```powershell
cargo test --locked --offline -j2 -p localvox-light-terminal
cargo test --locked --offline -j2 -p localvox-light-terminal --test pty_workspace
cargo build --locked --offline --release -j2 -p localvox-light-terminal --bin localvox-tui
cargo fmt -p localvox-light-terminal -- --check
```

- Library **145 passed / 2 ignored**, 10.51 s; все существующие visual snapshots
  прошли без изменения baselines. [Пять новых тестов](../../../../crates/localvox-light-terminal/src/app/paging/tests.rs)
  проверили реальный TestBackend: 80×24/120×36/160×48 дают 13/25/37 строк документа
  и 7/9/15 двухстрочных записей на страницу; фильтр вычитает свою нижнюю строку.
  Проверены переносы, resize, границы/End, выбор+offset целой страницы, одно-/двухстрочные
  списки, фильтр, журнал/follow, нулевая/маленькая область и отсутствие player/I/O effects.
- Полный TUI integration: IO **2 passed**, run-script/clean bundle **2 passed**.
  Прежний real mpv media test ignored; звуковой backend не менялся.
- Дополнительно расширенный [ConPTY](../../../../crates/localvox-light-terminal/tests/pty_workspace.rs):
  **4 passed**, 15.54 s. Настоящий клиент/temporary HTTP: 80 записей, Page Down/Up
  передвигают выбор 00↔09, Enter открывает документ, Page Down показывает строку 023
  после заголовков; после resize 120×36→80×24 следующий Page Down показывает 036.
  Сохранены прежние проверки activity/filter/RU input/auth/terminal cleanup.
  Фикстуры не используют устройства, модели или пользовательский архив.
- Release build **passed**, 12.75 s, `--version` passed. Бинарник
  `target/release/localvox-tui.exe`, SHA256
  `cd01e59165e8301d58eebd8133f499cfff2f44d90277dc656aee55b6e7b9dbcd`.
  Рабочий daemon/worker не останавливались/не пересобирались.
- Обновлены US-011, terminal guide и [короткая ручная проверка](../../../../docs/incremental-acceptance.md).

## Traceability

| ID | Delta / код | Свидетельство и граница |
|---|---|---|
| ARCH-013 | [контракт](../specs/archive-playback/spec.md), [paging](../../../../crates/localvox-light-terminal/src/app/paging.rs), [геометрия](../../../../crates/localvox-light-terminal/src/render/geometry.rs) | 5 unit/render + 4 ConPTY, прежние snapshots; Windows, реальный звук и Mac not-run |

Новый ID остаётся delta, main specs не синхронизированы. Strict main: 15 passed /
0 failed; strict change valid; check-openspec: 88 requirements, 188 scenarios,
88 coverage rows, 1736 links PASS. Fmt и diff check passed; удалённый CI не запускался.
Предыдущие performance/default/hardware gates ранней STT этим изменением не закрыты.
