# Tasks

## 1. Контрольная точка и границы

- [x] 1.1 Записать карту всех US-сценариев, владельцев/портов/адаптеров и проверок; сверить полноту с user-guide и факт существования модулей/тестов. Отделить исторические доказательства от новых прогонов и открытых hardware/default/BL-006 пунктов.

## 2. Клиенты и прикладные операции

- [x] 2.1 Собрать TUI preferences и reset/reconnect, отделить input/data responses/render modules; пройти TUI library/ConPTY/IO/run-script с INSTA_UPDATE=no, сохранением snapshots и tests preferences/reset/input boundary.
- [x] 2.2 Разделить React document views/controller и Archive API по feature operations при прежних DTO/HTTP/DOM/copy; UI build/unit/browser e2e и API regression подтверждают поведение, обновить карту владельцев.

## 3. Обработка и сменяемые реализации

- [ ] 3.1 Отделить worker execution от queue/claim/acceptance и общие import/config builders от CLI/HTTP glue, сохранив policies старых входов; real coordinator/worker fault/legacy/cancel/retry tests подтверждают ownership и артефакты, BL-006 не закрывать без его полного критерия.
- [ ] 3.2 Изолировать ASR backend construction/dependencies и LLM transport от сценариев; сохранить native/fixture identity и HTTP/CLI requests/options/errors. Adapter/fake-backend tests, ASR fixtures/native smoke и LLM regressions проходят; обновить архитектурные границы.

## 4. Очистка и завершение

- [ ] 4.1 Проверить legacy/дублирования/описания настроек после переноса, удалить только доказанно лишнее; compatibility/настройки tests и diff подтверждают отсутствие новых действий/форматов/алгоритмов. User-guide менять только при изменении поведения; намеренные изменения вынести отдельно.
- [ ] 4.2 Пройти существенные общие Rust/UI/browser/ConPTY regressions, strict main/change/check-openspec/scoped rustfmt/diff; собрать согласованный release/review bundle, обновить evidence/coverage и коммитить проверенные этапы. Hardware/Mac not-run, push не выполнять.
