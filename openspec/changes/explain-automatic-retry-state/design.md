# Design

## Context

См. proposal/Why. Workflow::execution проверяет coordinator только для Pending/Running, у Failed выводит retry_wait по наличию automatic_retry_at. Queue DTO не содержит phase/план. Scheduler умеет повторять legacy Failed без failed_at, поэтому отсутствие даты не означает исчерпанный бюджет. Текущая политика и её ограничения остаются PIPE-010.

## Goals / Non-Goals

Единое объяснение автоматического повтора в очереди и «Ход», без проверки всех артефактов для каждой строки queue. Не менять scheduler, записи jobs/progress, resume/recook, интервалы или число попыток. Не реализовывать BL-002/004 и не запускать модели.

## Decisions

- Чистый helper workflow::retry_plan(job, coordinator, now) создаёт план только для Failed: state/message/retry_at. Лимит MAX_REVIVALS и дата automatic_retry_at остаются источником политики. Вычислять эту логику отдельно в двух клиентах означало бы повторить расхождение.
- Свежий ready подтверждает retry_wait; starting/disabled/blocked/stopped дают blocked с причиной, stale/некорректный heartbeat — stalled, None — blocked с неизвестным координатором. Исчерпанный бюджет всегда failed с ручным повтором. Нет failed_at при бюджете — неизвестная дата, возможность автоматического возвращения не отрицается.
- Queue получает additive phase и optional retry plan; один snapshot coordinator и now на весь ответ. Успешный старый payload и порядок сохраняются. Progress использует тот же helper, сохраняя текущие stage/artifact/time проверки и last_error.
- UI показывает phase/attempts и план рядом с ошибкой, TUI дополняет документ выбранного задания и индикатор в списке. Старый API без новых полей обозначается неизвестным сроком/фазой; клиент не угадывает календарь. Не нужен второй поллинг или чтение артефактов для queue.
- Delta основана на полном PIPE-011, включая сценарии fix-queue-read-failures, чтобы не потерять уже уточнённый контракт при дальнейшем sync. Main specs и предыдущие deltas сейчас не меняются.

## Risks / Trade-offs

- Глобальный coordinator усложняет детерминированные проверки → core unit передают явный snapshot/clock, API tests не меняют глобальное состояние процесса.
- Запоздалый snapshot/ошибка сети → прежние stale/error маркеры и заморозка таймеров остаются, абсолютная дата не превращается в обещание запуска.
- Чтение очереди не должно стать дороже проверки каждой сессии → helper не открывает файлы и не запускает workflow::view по всем строкам.

## Migration Plan

Миграции нет. Новые поля nullable/additive, старые JSON читаются, новые UI/TUI требуют свежего API для полных сведений. Откат — пересборка старых исходников без изменения архива.

## Validation

Сначала тест Failed + disabled coordinator воспроизводит ложный retry_wait. Затем matrix ready/starting/disabled/stale/None, missing date/exhausted, 1/2/4 часа и неизменность данных; настоящий API queue/progress, старые DTO, UI/TUI текст/стили и переходы без мутаций. Полные browser/Rust проверки по риску, общий temp API/UI/TUI, строгие main/change/check-openspec/format/diff. TEMP/TMP процессов на D:.
