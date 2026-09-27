# Transcript versions Specification

## Purpose

F8/F1. Рабочая версия, язык STT, refined, чтение и повторные обработки.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#transcript-versions).

## Requirements

### Requirement: VER-001 Версии и best

VersionStore SHALL выделять последовательные ID и файлы vNNN-label.jsonl, хранить model/parents/recipe в versions.json и отвергать commit без файла или с дублирующимся ID. При отсутствии явно выбранного best используется последняя версия. Повреждённый манифест SHALL не заменяться пустым при изменении версий; запись использует строгую загрузку. Чтение для отображения и доказательство целостности — разные операции; отсутствие доступной версии при повреждении не доказывает отсутствие сохранённого текста.

Код: [VersionStore](../../../crates/localvox-light-core/src/versions.rs).
Существующая проверка: `commit_requires_existing_file`, `commit_rejects_duplicate_id`, `best_defaults_to_latest_until_set`.

#### Scenario: Несуществующий результат

- **WHEN** commit ссылается на отсутствующий файл
- **THEN** версия не регистрируется.

#### Scenario: Выбор старой версии

- **WHEN** set_best получил существующий ID
- **THEN** рабочей становится эта версия.

### Requirement: VER-002 Язык выбирает модель

ASR SHALL использовать язык сессии из meta, иначе ru; автоматическое определение текста не выбирает акустическую модель. Глобальный LOCALVOX_LANG фиксируется для новых сессий. Для текста используется explicit, затем lang_detected, затем ru. Надёжное text-detect требует >=20 слов; ISO-коды нормализуются.

Код: [asr, text, explicit, detect, normalize](../../../crates/localvox-light-core/src/lang.rs), [create_session_dir](../../../crates/localvox-light-core/src/chunks.rs).

#### Scenario: Смена глобального языка

- **WHEN** LOCALVOX_LANG меняется после создания старой сессии
- **THEN** asr старой сессии не читает новый global как свой explicit.

#### Scenario: Короткий текст

- **WHEN** передано меньше двадцати слов детектору
- **THEN** detect возвращает None, а не уверенное определение.

### Requirement: VER-003 API смены языка и best

API SHALL до смены языка проверить пригодность для recook и наличие модели указанного языка, затем сохранить язык и запросить полный recook; одинаковый язык не вызывает повтор. set_best только меняет указатель в VersionStore: это не синхронная генерация новой сводки.

Код: [Archive](../../../crates/localvox-light-api/src/archive.rs).

#### Scenario: Нет модели языка

- **WHEN** пользователь выбирает неподдержанный установленными весами язык
- **THEN** ошибка возникает до удаления производных этой веткой.

#### Scenario: Только best

- **WHEN** пользователь выбирает существующую версию
- **THEN** указатель меняется без немедленного LLM-вызова.

### Requirement: INV-VER-001 Текст и имена имеют разную изменяемость

Refine SHALL создавать дочернюю версию с сохранёнными строками/таймкодами, не подменяя исходный STT-текст. Переименование говорящего relabel_speaker, напротив, переписывает speaker во всех версиях. Хеш для обработки исключает изменяемые имена, поэтому «все версии побайтово неизменны» неверно.

Код: [process_session, refine_session](../../../crates/localvox-light-llm/src/pipeline.rs), [relabel_speaker](../../../crates/localvox-light-core/src/versions.rs), [capture](../../../crates/localvox-light-core/src/artifacts.rs).
Существующая проверка: `naming_a_voice_renames_it_in_every_version`.

#### Scenario: Назван участник

- **WHEN** relabel_speaker меняет метку
- **THEN** меняется speaker, не текст распознавания и не таймкоды.

### Requirement: VER-004 Повторы по scope

API SHALL различать Summary/Text/All; recook получает .worker.lock и принимает задание до инвалидирования выбранных производных. All SHALL требовать полного пригодного аудио; Text/Summary SHALL работать без аудио при валидном сохранённом транскрипте. Отсутствующий/повреждённый транскрипт SHALL давать ошибку, не скрытый запуск STT. CLI --post-only сохраняет работу по существующему тексту без STT.

Код: [Archive](../../../crates/localvox-light-api/src/archive.rs), [main](../../../crates/localvox-light-asr/src/bin/localvox-process.rs).
Проверено: `text_and_summary_recook_without_audio_start_at_the_requested_phase`,
`full_recook_without_audio_preserves_the_last_results`,
`postprocess_rejects_missing_malformed_and_unconfirmed_empty_transcripts`,
`recook_does_not_invalidate_artifacts_owned_by_a_live_worker`.
Общая строгая проверка источника: `artifacts::source_transcript`; legacy без receipts
принимается структурно, имеющиеся receipts проверяются. [Evidence](../../evidence/implementation-review-2026-09-27.md).

#### Scenario: Аудио истекло

- **WHEN** API recook Summary или Text запрошен после retention при валидном тексте
- **THEN** операция принимается без повторного STT, исходная версия сохраняется.

#### Scenario: Полный повтор без аудио

- **WHEN** All запрошен без доступного аудио
- **THEN** API объясняет отсутствие исходника и не инвалидирует сохранённый текст.

#### Scenario: Нет валидного текста

- **WHEN** Text/Summary запрошен без валидного транскрипта и без аудио
- **THEN** операция возвращает ошибку без уничтожения данных.

#### Scenario: Занятый worker

- **WHEN** recook выполняется при удерживаемом .worker.lock
- **THEN** отказ не уничтожает живой результат.

## Implementation status

BL-002: общий lease есть, общей транзакции нескольких файлов нет; legacy load для чтения может скрывать повреждение как пустой список. BL-007 проверен без реального ASR.

[Матрица по требованиям](../../evidence/coverage.md#transcript-versions) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

BL-002 выполнен частично: recook/best/имена/язык/delete теперь берут единый execution lock до изменений, set_lang восстанавливает meta при отказе постановки. Это не crash-транзакция нескольких файлов; управляемая отмена/удаление занятой обработки ещё не реализованы. BL-007 проверен без настоящей модели; реальные модели разных языков не прогонялись.
