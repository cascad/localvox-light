# Design

## Context

См. proposal/Why. QueuePane.retry вызывает api.recook без scope. В Archive это All: проверка audio, Prepare/force и удаление производных. В TUI есть только три явных recook-команды. Core умеет автоматический bounded retry той же фазы, но отдельной команды ручного возобновления нет.

## Goals / Non-Goals

Исправить семантику повтора очереди, сохраняя явный full recook. Не реализовывать автоматический ремонт Done (BL-004), controlled cancel/delete (BL-002), восстановление потерянного HTTP acknowledgement или exactly-once модельных вызовов.

## Decisions

- Новый POST /api/sessions/{name}/retry и методы клиентов, без изменения старого recook: переписать default recook означало бы сломать намеренный полный STT и старых клиентов.
- Core retry_failed изменяет только Failed того же id/phase/kind и сохраняет параметры. Сбрасывает attempts/revivals, старые execution/started/finished/error и force, задаёт новые enqueued/phase_queued часы. При save failure восстанавливает snapshot в памяти. Повтор Pending не принимается и не обновляет время.
- Archive берёт существующий session lease, затем короткий mutex очереди; проверяет read/save и состояние, публикует новую границу progress только после принятия. Артефакты/receipts не удаляются. Новая граница не стирает старую историю. Полная crash-транзакция queue+progress не заявляется.
- UI кнопка «Повторить этап» блокируется на время POST, после успеха явно обновляет очередь/счётчики. В TUI — именованное действие «Повторить упавший этап», R вне форм на выбранной Failed-строке очереди либо текущей архивной сессии; повторная отправка блокируется существующим pending action. Явные scoped recook сохраняют прежнее подтверждение.
- Поддержка старого API: отсутствующий retry показывает ошибку. Откат к full recook недопустим, он уничтожил бы смысл операции.
- TUI отмечает pending retry отдельно от инвалидирования документа: после Action acknowledgement меняет epoch и обновляет чтения, сохраняя готовые документы. Ответ Control не забирает эту отметку; stop записи остаётся независимым.

## Risks / Trade-offs

- Ручной повтор и worker могут встретиться → session lease и свежая проверка Failed под mutex; тесты с held lease, Pending/Done и двойным POST.
- Частичный результат упавшей фазы может быть уже годным → force=false разрешает проверку/переиспользование; неисправный результат не принимается одним exit code.
- Следующий этап автоматического job учитывает live summary policy → follow_settings сохраняется, явные производные остаются snapshot. Конфигурация worker по-прежнему фиксируется при старте.
- Запись новой границы progress best-effort как у прежнего recook → queue timing остаётся источником нового запуска; общей гарантии при power loss нет.

## Migration Plan

Данные совместимы. Сборка UI/API/TUI обычными командами; новая кнопка требует обновлённого API. Откат возвращает исходники/сборку, архив миграции не требует.

## Validation

До исправления browser POST из кнопки очереди должен воспроизвести recook без scope. После: core четыре фазы и разные kind/settings, rollback отказа сохранения; настоящий HTTP/client без аудио на Summary/Prepare, байты артефактов/receipts, новая очередь/граница, held lease/recording/corrupt queue/не Failed/двойной POST. UI pending/error/retry без recook и TUI выбранный session/R/палитра/формы. Полные релевантные core/API/client/TUI/browser проверки и общий temp API/UI/TUI, OpenSpec и diff; TEMP/TMP процессов на D:.
