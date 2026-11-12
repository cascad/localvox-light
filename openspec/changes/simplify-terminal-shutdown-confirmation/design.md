# Design

## Context

См. proposal. Сейчас shutdown использует общую форму с вводом ДА.
Штатные Ctrl+S/Ctrl+Enter и Esc уже обрабатываются до редактора полей.

## Decisions

Отдельный FormKind::Shutdown без полей: короткий renderer с предупреждением и
подсказкой. Существующий submit отправляет прежний Task::Shutdown и сохраняет
pending/error/retry bookkeeping. Редактор и paste безопасно пропускают формы без
полей. Не прятать фиктивное поле и не подставлять ДА программно.

## Risks / Trade-offs

Ввод/Tab/paste не должен обращаться к отсутствующему полю: проверить реальные
key/paste paths. Остальные typed confirmations сохраняют их прежний контракт.
Новый native ConPTY сценарий подтверждает пустой диалог одним Ctrl+S.
