# Ручная обратная связь — 2026-11-02

Выполнены 12.1/12.2. Предустановки/LLM-адреса имеют отдельный
[change и результаты](../../refine-workspace-controls-and-settings/evidence/implementation.md).

| Требование | Изменение | Проверка и результат |
|---|---|---|
| JRN-003 | Ручное раскрытие/сворачивание сохраняет внутренний и внешний scroll; follow выключается до явного «к хвосту». Отложенный polling не отменяет ручное действие | [browser e2e](../../../../ui/tests/e2e/workspace.spec.ts), `expanding journal details preserves both scroll positions until explicit follow`: expand/poll/collapse и явный tail — PASS |
| TERM-005 | Фиксированная ширина значка play/pause/stop, позиция выровнена по длительности. F6 передаёт стрелки плееру и возвращает документу | [render](../../../../crates/localvox-light-terminal/src/render/ux_tests.rs): одинаковая колонка волны при play/pause и переходе через 100 минут; [keyboard](../../../../crates/localvox-light-terminal/src/app/ux_tests.rs): F6 → seek → F6 → документ — PASS |
| TERM-006 | Короткие маркеры «значение / применение / пояснение», акцент значения, приглушённая подсказка и источник .env/.env.defaults | 80×24 light, 120×36 dark и новый preset snapshot — PASS; UI preset screenshot просмотрен |
| TERM-007 / JRN-006 | Убрана декоративная «технический журнал» из шапки; пояснены F6/значки в помощи и статусе | Обновлённые snapshots, keyboard tests, справочник US-015/036/049 — PASS |

Полный browser прогон: **76 passed** (25.6 s), UI unit **10 passed**, build PASS.
Финальный TUI lib прогон: **125 passed, 2 ignored**, включая 43 render tests; обычный
прогон использовал `INSTA_UPDATE=no`. Предыдущий полный client/TUI прогон также прошёл
transport/ConPTY. API **76 passed, 6 ignored**, приватный settings HTTP повторно PASS.

Это автоматическая проверка переходов/геометрии и просмотр golden, не аппаратная
приёмка владельца. Старые результаты не переименованы. Задачи **10.2 и 11.3 открыты**:
физический звук/захват и исходный слышимый отказ mpv не проверены этим этапом.
Сборка release через `scripts/run.ps1 -Tui -BuildOnly` успешна, приложения не запускались.
