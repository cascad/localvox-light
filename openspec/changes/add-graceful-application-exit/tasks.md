# Tasks

## 1. Runtime

- [x] 1.1 Добавить application control и HTTP stop, подключить результат штатного daemon cleanup; проверить isolated owner/HTTP tests: auth/origin/confirm/unavailable, success/failure/timeout/drain и отказ новых mutations.
- [x] 1.2 Добавить один bounded client POST с проверкой completed; проверить endpoint, lost ack/no retry и некорректный success без настоящего демона.

## 2. TUI

- [x] 2.1 Добавить отдельную команду/подтверждение с адресом, ожидание и обработку результата, сохранить client-only quit; проверить cancel/success/error/busy/reconnect unit и ConPTY, обновить US-062 и guide.
- [x] 2.2 Выполнить relevant app/API/client/TUI integration/build и spec checks, evidence/coverage и локальный commit; hardware/Mac оставить not-run.
