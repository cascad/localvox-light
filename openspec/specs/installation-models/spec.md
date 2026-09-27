# Installation and models Specification

## Purpose

Установка, модели и диагностика продукта из README, сверенные со скриптами и загрузчиками.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#installation-models).

## Requirements

### Requirement: SETUP-001 Манифест загрузки

fetch-models SHALL использовать scripts/models.json, поддерживать выбор компонентов и required-only; Check проверяет наличие без загрузки. В каталоге required — Vosk/GigaAM, optional — diarize/NER; отсутствие optional снижает возможности соответствующего этапа. Список моделей и размеры в README не задают правила загрузчика.

Код: [manifest](../../../scripts/models.json), [fetch-models](../../../scripts/fetch-models.ps1), [fetch-models](../../../scripts/fetch-models.sh).

#### Scenario: Required only

- **WHEN** запрошен минимальный комплект
- **THEN** optional-компоненты пропускаются.

#### Scenario: Check

- **WHEN** включён режим проверки
- **THEN** веса не скачиваются.

### Requirement: INV-SETUP-001 Скачивание через временный файл

Скрипт SHALL скачивать в .part, сравнивать SHA-256, когда он задан, и переименовывать только после этой проверки. При mismatch временный файл удаляется и операция завершается ошибкой. null hash (Vosk archive) означает отсутствие такой проверки, а не успешно проверенную подлинность.

Код: [Get-Verified](../../../scripts/fetch-models.ps1), [fetch_verified](../../../scripts/fetch-models.sh).

#### Scenario: Неверный hash

- **WHEN** скачанные байты не совпали с ожидаемым SHA-256
- **THEN** файл не публикуется под готовым именем.

#### Scenario: Нет hash

- **WHEN** manifest содержит null с пояснением
- **THEN** загрузка не выдаётся за криптографически проверенную.

### Requirement: SETUP-002 Разрешение зависимостей

Штатные бинарники с Vosk SHALL использовать нативную библиотеку, требуемую загрузчиком ещё до main. Расположение ASR-модели выбирается из LOCALVOX_ASR_MODEL_DIR, соседнего models либо cwd; worker ищется рядом с daemon. Указание модели в другом месте не заставляет doctor проверять лишь установочный каталог.

Код: [native linking](../../../crates/localvox-light-core/build.rs), [default_model_dir](../../../crates/localvox-light-core/src/lang.rs), [Layout::from_env](../../../crates/localvox-light-core/src/doctor.rs).

#### Scenario: Модель задана явно

- **WHEN** существующий путь отличается от bundled models
- **THEN** диагностика использует путь разрешения модели.

### Requirement: SETUP-003 Doctor и границы проверки

Doctor SHALL собирать Layout, проверять доступные компоненты и выдавать Finding OK/Warn/Fail с пояснением и fix; exit code 0/1/2 различает полноту работоспособности. Это диагностика наличия/доступности, не полный прогон ASR и не измерение качества/скорости.

Код: [inspect](../../../crates/localvox-light-core/src/doctor.rs), [doctor entry](../../../crates/localvox-light/src/main.rs).

#### Scenario: Нет worker

- **WHEN** соседний localvox-process отсутствует
- **THEN** doctor сообщает недоступную фоновую обработку и способ исправления.

## Implementation status

Проверены Windows build и Check; скачивание/распаковка моделей заново, качество ASR и другие ОС не проверялись. Check не подменяет checksum и пробный inference.

[Матрица по требованиям](../../evidence/coverage.md#installation-models) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

Дистрибутив и загрузки моделей при миграции не запускались. Наличие файла в Check не тождественно полной повторной проверке всех весов; архивная распаковка и уже существующие файлы имеют отдельные ветки. Windows-first — факт текущей проверки, не запрет портирования; нативный GUI и capture каждой ОС требуют собственной приёмки. Установка/сборка по шагам остаются в README.
