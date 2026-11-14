# Tasks

## 1. Локальный preview для владельца

- [x] 1.1 Создать отдельный лёгкий frontend и безопасный demo fresh/existing, default/custom, Space checkbox/Enter/Esc, пути/итог. Проверить настоящий ConPTY keyboard flow/cancel/terminal restore, отсутствие side effects и PNG/анимацию; описать запуск на всех трёх ОС, strict validation и scoped diff, закоммитить локально.
- [x] 1.2 Владелец просматривает demo и подтверждает оформление/сценарий до подключения real install и GitHub build; сохранить результат просмотра, не считать технический тест человеческой приёмкой. Принято 2026-11-14: «Ну круто, мне нравится 2 вариант».
- [x] 1.3 Учесть отзыв владельца: явный возврат назад, сохранение подтверждённых ответов, корректный дальнейший маршрут после правок. Проверить ConPTY, обновить демо/снимки/справочник и закоммитить; оформление владелец одобрил, проверка навигации остаётся открытой.

## 2. Подключение реальной установки после просмотра

- [x] 2.1 Подключить общий frontend к detection/plan/execute, убрать ложную missing .env ошибку, сохранить реальные parse/access errors, reuse/default/custom/Ollama behavior; проверить в private subprocess/TTY/HTTP fixtures и обновить user-guide/README. [Результаты](evidence/integration.md).
- [x] 2.2 Добавить bootstrap одной строкой Win/Linux/Mac без ручного binaries/jq/Rust, согласованные release version/hash/target и типовую установку зависимостей; проверить dry fixture stand для трёх ОС без настоящих weights/installers и дать владельцу native Linux/Mac команды стенда. [Команды](../../../docs/installer-testing.md); Windows и четыре Unix target routes проверены локально, native Unix stand остаётся not-run.

## 3. Доставка

- [ ] 3.1 Выполнить затронутые тесты/полный Windows bundle smoke и strict specs/changes/evidence, подготовить полный release workflow и one-liner README; запросить отдельное разрешение только на concrete publication/push, native hardware/непройденные ОС оставить not-run.

Локальная реализация, Windows package smoke, README и workflow подготовлены;
новые hosted builds/publication и native Unix остаются отдельной границей приёмки.
Без push/tag новый публичный one-liner не объявляется доступным.
