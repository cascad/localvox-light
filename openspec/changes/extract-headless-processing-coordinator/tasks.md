# Tasks

## 1. Общий координатор

- [x] 1.1 Экспортировать autocook через lib существующего пакета и подключить main без копии реализации; выполнить прежние coordinator/lib и launcher/bin tests, обновить CI/docs/evidence с границами переноса.
- [x] 1.2 Добавить optional bounded metadata observer после принятых переходов; проверить exact clocks/исходы, отсутствие событий при save failure, отсутствие raw error/text и неблокирующий overflow/lost counter; обновить документацию/evidence.

## 2. Headless стенд

- [x] 2.1 Добавить queue runner с marker/recording/worker guards, metadata report, terminal/timeout/stop lifecycle; выполнить model-free tests и настоящий empty/preflight smoke без устройств, описать команды и ограничения.

## 3. Совместимость

- [x] 3.1 Выполнить затронутые core/root/lib/bin/launcher и сборочные проверки, strict main/change/check-openspec/format/diff, обновить coverage/catalog/evidence; закоммитить только проверенный срез, оставив BL-003 и аппаратную приёмку открытыми.
