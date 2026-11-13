# Tasks

## 1. Platform path regressions

- [x] 1.1 Воспроизвести alias Hidden, исправить Windows normalization, Unix physical ZIP guard и curl Unicode output; добавить regression fixtures для aliases, final model symlink, native Windows curl, Check/repeat/existing attrs; запустить PS 5.1/7.x и Git Bash, сохранить evidence.
- [x] 1.2 После hosted 99bd709 нормализовать обе стороны prefix check также в Bash new_model_parent; воспроизвести HOME alias Hidden=0 с прежним helper, проверить новый/готовый user root через NTFS junction и Unix alias без изменения чужих атрибутов, записать evidence.

## 2. Integration

- [x] 2.1 Перепаковать текущие release binaries со свежими helpers, прогнать installed model bridge/doctor/setup/reconcile и delivery; strict specs/change/check-openspec/actionlint/diff, обновить coverage/evidence и закоммитить только исправление. Native Mac/new hosted оставить not-run до нового push владельца.
- [x] 2.2 Перепаковать Bash fix с теми же binaries, выполнить model-storage и installed bundle doctor/setup/reconcile, strict validation/check-openspec/actionlint/diff и закоммитить дополнение; отделить hosted Linux/Mac/Windows результаты от новых локальных проверок.
