# Vim-навигация · 2026-11-11

Уточнение владельца: l/д открывает выбранную видимую запись как Enter только вне
ввода в списке архива; h/l (р/д) при фокусе документа вызывают существующие Left/Right.
Поиск/формы сохраняют исходный текст, меню/плеер/прочие панели и модификаторы
не получают новых привязок. US-011 и помощь обновлены; snapshots не менялись.

`cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`:
164 library passed / 3 fixture helpers ignored, 2 IO / 4 ConPTY / 2 run-script
passed, hardware mpv ignored. Новые тесты проверяют выбор из сохранённого фильтра,
RU/EN, пустой список, буквальность ввода, журнал, другие панели/модификаторы/палитру.
Main specs strict: 15 passed; change strict, check-openspec и diff прошли.

Release TUI скомпилирован/связан, установка в обычный exe заблокирована открытым
клиентом (Access denied): cargo build не объявляется успешной полной командой.
Проверенный linked artifact скопирован в
`target/review/vim-navigation/localvox-tui.exe`, `--version` проходит; свежая строка
помощи найдена внутри кандидата. SHA-256:
`f8131a9ec8fbc4fe58f2068ffa8e33aa62014ab07a6978591cc82a93ad78a394`.
Открытый TUI и daemon не завершались, архив/настройки/буфер владельца не менялись.
Ручная приёмка — not-run. ARCH-011/012 остаются delta; sync/archive не выполнялись.
