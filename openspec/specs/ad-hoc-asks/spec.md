# Ad hoc asks Specification

## Purpose

F1/F6. Раздел «Спросить» о произвольном материале, отдельный от RAG-чата и видеоочереди.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#ad-hoc-asks).

## Requirements

### Requirement: ASK-001 Приём материала

create_ask SHALL отвергать пустой материал и более 400000 Unicode-символов, выбирать default prompt при пустом пользовательском, сохранять input.txt и ask.json со статусом Pending до ответа. Провайдер SHALL быть явно выбран для операции либо взят из ранее сохранённого выбора пользователя (DEC-006). При отсутствии такого выбора API SHALL запросить выбор, не назначать облако неявно; неизвестное имя провайдера SHALL отклоняться. UI показывает выбранный провайдер до отправки. Ошибка выбранного локального клиента не разрешает скрытый переход к облачному. Конкретный новый default этим контрактом не устанавливается.

Код: [Archive](../../../crates/localvox-light-api/src/archive.rs), [effective_prompt, save](../../../crates/localvox-light-core/src/asks.rs), [AskPane](../../../ui/src/Ask.tsx).
Существующая проверка: `effective_prompt_falls_back_to_default`, `save_and_load_roundtrip_and_files_split_by_mutability`.

#### Scenario: Пустой материал

- **WHEN** text состоит из пробелов
- **THEN** запрос не создаётся.

#### Scenario: Выбор файла

- **WHEN** файл перетащен в UI
- **THEN** используется File.text(), специального извлечения PDF/DOCX нет.

#### Scenario: Провайдер не выбран

- **WHEN** в API не передан provider и нет сохранённого явного выбора
- **THEN** задание не принимается до выбора; материал не отправляется облачному провайдеру.

#### Scenario: Неизвестный провайдер

- **WHEN** provider не соответствует поддерживаемому выбору
- **THEN** API сообщает ошибку до создания задания вместо подстановки другого клиента.

### Requirement: ASK-002 Отдельная очередь

Worker asks SHALL выбирать старейший Pending, сохранять Running до вызова LLM, затем Done с answer либо Failed с ошибкой. При старте Running возвращаются в Pending. Обновление не переписывает input.txt; история сортируется новыми вперёд и пропускает нечитаемые ask.json.

Код: [Archive](../../../crates/localvox-light-api/src/archive.rs), [pending, reclaim_running, update, list](../../../crates/localvox-light-core/src/asks.rs).
Существующая проверка: `a_failed_ask_is_saved_and_marked`, `pending_is_fifo_and_reclaim_revives_running`.

#### Scenario: Ошибка провайдера

- **WHEN** модель вернула ошибку
- **THEN** материал остаётся, Failed и причина сохраняются.

#### Scenario: Рестарт

- **WHEN** запрос остался Running
- **THEN** он вновь ожидает выполнения и может повторно вызвать внешнюю модель.

### Requirement: ASK-003 История и удаление

UI SHALL опрашивать историю при незавершённых запросах и показывать сохранённый ответ после возврата в раздел. Удаление asks убирает input/answer/record; отсутствующий каталог не считается ошибкой. UI требует второй клик, но API не использует session confirm-token.

Код: [delete](../../../crates/localvox-light-core/src/asks.rs), [AskHistory, AskRow](../../../ui/src/Ask.tsx).
Существующая проверка: `delete_removes_the_folder_and_is_idempotent`.

#### Scenario: Повтор удаления

- **WHEN** каталог уже отсутствует
- **THEN** delete возвращает успех.

## Implementation status

ASK-001 нарушен в API: отсутствие provider сохраняет Claude, неизвестное имя выбирает обычный клиент (RV-009). Сохранение не является общей crash-транзакцией, удаление Running не согласовано с worker. Реальный провайдер не вызывался.

[Матрица по требованиям](../../evidence/coverage.md#ad-hoc-asks) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

Asks не используют фазовые receipts и общий scheduler PIPE. save/update пишут файлы напрямую, а не атомарным commit нескольких файлов; crash-consistency и удаление Running не подтверждены. Не объявлять сырой ответ проверенным grounding сводки. RV-009: глобальное обещание локального default несовместимо с default claude этого раздела.
