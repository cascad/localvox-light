# Tasks

## 1. Локальный preview для владельца

- [x] 1.1 Создать отдельный лёгкий frontend и безопасный demo fresh/existing, default/custom, Space checkbox/Enter/Esc, пути/итог. Проверить настоящий ConPTY keyboard flow/cancel/terminal restore, отсутствие side effects и PNG/анимацию; описать запуск на всех трёх ОС, strict validation и scoped diff, закоммитить локально.
- [x] 1.2 Владелец просматривает demo и подтверждает оформление/сценарий до подключения real install и GitHub build; сохранить результат просмотра, не считать технический тест человеческой приёмкой. Принято 2026-11-14: «Ну круто, мне нравится 2 вариант».
- [x] 1.3 Учесть отзыв владельца: явный возврат назад, сохранение подтверждённых ответов, корректный дальнейший маршрут после правок. Проверить ConPTY, обновить демо/снимки/справочник и закоммитить; оформление владелец одобрил, проверка навигации остаётся открытой.

## 2. Подключение реальной установки после просмотра

- [x] 2.1 Подключить общий frontend к detection/plan/execute, убрать ложную missing .env ошибку, сохранить реальные parse/access errors, reuse/default/custom/Ollama behavior; проверить в private subprocess/TTY/HTTP fixtures и обновить user-guide/README. [Результаты](evidence/integration.md).
- [x] 2.2 Добавить bootstrap одной строкой Win/Linux/Mac без ручного binaries/jq/Rust, согласованные release version/hash/target и типовую установку зависимостей; проверить dry fixture stand для трёх ОС без настоящих weights/installers и дать владельцу native Linux/Mac команды стенда. [Команды](../../../docs/installer-testing.md); локальные fixtures и затем native CI всех пяти targets прошли, ручная аппаратная приёмка отдельно.

## 3. Доставка

- [x] 3.1 Выполнить затронутые тесты/полный Windows bundle smoke и strict specs/changes/evidence, подготовить полный release workflow и one-liner README; запросить отдельное разрешение только на concrete publication/push, native hardware/непройденные ОС оставить not-run. Владелец отправил `v0.2.0` на `4514678`; все targets и публикация прошли. [Проверка публичной доставки](evidence/release-v0.2.0.md).

Доставка завершена: публичный релиз, bootstrap/checksums и native CI подтверждены.
Windows one-liner проверен до настоящего мастера и отменён без установки.
Реальная установка всех зависимостей/больших моделей и аппаратная запись остаются
ручной приёмкой; завершение доставки не выдаёт их за пройденные проверки.
