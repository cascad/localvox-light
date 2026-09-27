# LLM processing Specification

## Purpose

F1. Уточнение текста, читаемые строки, шаблоны, сводки, проверки и провайдеры. Порядок фаз — PIPE-005/006/007.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#llm-processing).

## Requirements

### Requirement: LLM-001 Отсутствие речи и короткая запись

process_session SHALL при нуле распознанных слов пропускать LLM, удалять устаревший результат и возвращать skipped с причиной. Непустая короткая запись получает note-шаблон, длинная — summary; явно заданный непустой summary_template приоритетнее выбора по длине/языку. Cleanup использует свой встроенный prompt строк.

Код: [process_session, refine_session](../../../crates/localvox-light-llm/src/pipeline.rs).
Существующая проверка: `babbling_has_no_words_at_all`, `a_short_thought_is_real_speech_and_gets_the_note_shape`.

#### Scenario: Тишина

- **WHEN** speech_words равен нулю
- **THEN** сводка из тишины не запрашивается у модели.

#### Scenario: Явный шаблон

- **WHEN** короткая запись имеет явно выбранный шаблон
- **THEN** он сохраняет приоритет над автоматическим note.

### Requirement: LLM-002 Глоссарий и шаблоны

Система SHALL загружать глоссарий, выполнять детерминированные замены и добавлять релевантный glossary block в prompt. Канонические термины учитываются grounding. Шаблоны загружаются по имени/языку, с пользовательским каталогом; ошибки загрузки являются ошибками обработки, а не фиктивной готовностью.

Код: [process_session, refine_session](../../../crates/localvox-light-llm/src/pipeline.rs), [for_lang, load](../../../crates/localvox-light-llm/src/templates.rs), [Glossary](../../../crates/localvox-light-llm/src/glossary.rs).
Существующая проверка: `glossary_canon_is_grounded_in_the_final_check`.

#### Scenario: Исправленный термин

- **WHEN** глоссарий заменил термин каноническим
- **THEN** финальная проверка учитывает этот канон как допустимый источник.

### Requirement: LLM-003 Refine и cleanup по строкам

Refine SHALL выдавать новую версию; cleanup — processed.json с source version и поправками строк. Размеры пакетов ограничиваются числом строк и символами; одна слишком длинная строка идёт отдельно. Имена/разметка/времена берутся из транскрипта, не сочиняются cleanup-моделью; пропущенная или отвергнутая поправка сохраняет исходную строку.

Код: [process_session, refine_session](../../../crates/localvox-light-llm/src/pipeline.rs), [save](../../../crates/localvox-light-core/src/readable.rs).
Существующая проверка: `an_essay_instead_of_the_lines_costs_the_cleanup_not_the_text`, `lines_the_model_never_answered_for_keep_their_own_wording`, `one_oversized_line_goes_alone_rather_than_in_half`.

#### Scenario: Модель написала эссе

- **WHEN** ответ не содержит допустимых поправок строк
- **THEN** исходный текст не заменяется эссе.

#### Scenario: Пропущенная строка

- **WHEN** модель не вернула поправку на строку
- **THEN** в читаемом результате остаётся исходная формулировка.

### Requirement: LLM-004 Сводка и ограничения проверки

Summary SHALL обрабатывать части, при нескольких выполнять reduce, затем проверять содержание и ссылки, убирать повторяющийся хвост и пустые секции. Числа проверяются детерминированно, имена — с доступным NER. Необоснованная ссылка может быть удалена с сохранением самого утверждения; сомнительный документ сохраняется с unverified, не считается математически доказанным.

Код: [process_session, refine_session](../../../crates/localvox-light-llm/src/pipeline.rs), [check, SourceEntities](../../../crates/localvox-light-llm/src/grounding.rs).
Существующая проверка: `a_doubtful_result_is_still_written_as_the_real_document`, `a_citation_that_does_not_check_out_is_dropped_but_the_claim_stays`, `a_looping_summary_is_cut_at_its_first_repetition`.

#### Scenario: Сомнение

- **WHEN** результат содержит непроверенное содержание
- **THEN** документ остаётся доступен с соответствующей пометкой.

#### Scenario: Повтор текста

- **WHEN** модель зациклила сводку
- **THEN** повторяющийся хвост обрезается.

### Requirement: LLM-005 Выбор провайдера и Claude CLI

Провайдер SHALL выбираться явно для операции или из сохранённого выбора пользователя (DEC-006); ошибка локального вызова SHALL не переключать его в облако. Обычный LLM-клиент SHALL использовать настроенный OpenAI-compatible endpoint/model; локальный Ollama — штатный путь. Явно выбранный Claude запускается через CLI с timeout и JSON-ответом; дочернему процессу удаляются ANTHROPIC_API_KEY и ANTHROPIC_AUTH_TOKEN. Выбор удалённого провайдера отправляет материал этому провайдеру; безусловное «ни байта наружу» неверно.

Код: [LlmClient](../../../crates/localvox-light-llm/src/lib.rs), [build_spec, run, parse_answer](../../../crates/localvox-light-llm/src/claude_cli.rs).
Существующая проверка: `api_key_vars_are_always_stripped_from_the_child`, `non_json_is_reported`, `empty_result_fails`.

#### Scenario: API key в окружении

- **WHEN** вызывается Claude subscription adapter
- **THEN** два указанных API-переменных не передаются дочернему процессу.

#### Scenario: Невалидный ответ CLI

- **WHEN** stdout не содержит корректный непустой результат
- **THEN** операция возвращает ошибку.

#### Scenario: Локальный провайдер недоступен

- **WHEN** выбранный локальный endpoint вернул ошибку
- **THEN** операция сообщает отказ; материал не отправляется альтернативному облачному провайдеру без выбора пользователя.

### Requirement: LLM-006 Подтверждение сомнения

API confirm SHALL отмечать согласие пользователя с содержанием артефакта, не заменяя проверку файла и источника в processing/artifacts. Пользовательская отметка и ready/invalid — разные состояния. Отказ проверки или записи подтверждения SHALL возвращаться как ошибка, а не как «пометки и так не было».

Код: [Archive](../../../crates/localvox-light-api/src/archive.rs), [confirm](../../../crates/localvox-light-core/src/processing.rs).
Существующая проверка: `confirming_a_doubt_removes_the_plaque_for_good` (проверяет снятие пометки, не все сочетания повреждений).

#### Scenario: Файл повреждён

- **WHEN** подтверждено содержание, но checksum не совпадает
- **THEN** само согласие пользователя не восстанавливает целостность артефакта.

#### Scenario: Отказ подтверждения

- **WHEN** confirm не может проверить checksum или сохранить пользовательскую отметку
- **THEN** API сообщает причину отказа и не подтверждает успех.

## Implementation status

LLM-006: API скрывает отказ confirm (RV-016); это дефект, а не разрешённый успех. Качество реальных моделей не принято; степень редакторской переформулировки сверх сохранения смысла требует отдельных примеров.

[Матрица по требованиям](../../evidence/coverage.md#llm-processing) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

Формулировка «нет галлюцинаций» не доказана эвристиками. Нет автоматического маскирования и подтверждённой private per-session политики запрета облака. Установленные провайдеры, лимиты подписок и качество реальных моделей в этой миграции не проверялись.
