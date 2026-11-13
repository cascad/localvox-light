# Tasks

## 1. Совместимое окружение

- [x] 1.1 Ввести общий Rust 1.99.1 pin и использовать его в обоих workflows; добавить regression checks от stable/drift, обновить developer guide; проверить actual Windows cargo/link с onnx на закреплённом compiler.
- [x] 1.2 Исправить shell lookup перед child PATH isolation; не пропускать Zsh/Fish tests. Проверить relevant registration/TUI tests и Unix compilation; native Mac оставить hosted not-run.

## 2. Поставка

- [x] 2.1 Собрать полный Windows комплект и пройти startup/install/doctor/reconcile без моделей и без пользовательских PATH/.env; выполнить delivery tests, strict specs/change/check-openspec/actionlint/diff и обновить evidence/coverage. Создать локальный commit; hosted rerun требует push владельца.
