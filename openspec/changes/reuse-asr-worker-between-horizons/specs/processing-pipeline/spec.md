# Spec Delta

## ADDED Requirements

### Requirement: PIPE-019 Повторное использование управляемого ASR

Последовательные задания одной Transcribe линии SHALL переиспользовать загруженную
ASR-модель при совпадении проверенной content identity, включая переход prefix→final.
Изменение модели/языка/рецепта SHALL исключать reuse несовпадающего engine; force
SHALL сохранять явное повторное распознавание. Между заданиями обработчик SHALL
не удерживать processing/capture/audio владение сессией и не менять fairness очереди.

Обработчик SHALL иметь ограниченные запросы/ответы/диагностику и время попытки;
cancel, timeout, protocol mismatch, crash и shutdown SHALL освобождать дерево
процессов и владение до дальнейшего изменения записи. Неподтверждённый ответ SHALL
не означать успеха. После ошибки восстановление SHALL принимать только валидные
артефакты. Keep-alive SHALL быть видимой настройкой LOCALVOX_ASR_KEEP_ALIVE_SEC
(предустановка 600 s, 0 выключает); простой SHALL освобождать память модели.

Повторное использование engine и устранение повторных обходов SHALL сохранять
покрытие PCM/результаты/кеш/гарантии commit и recovery. Старый performance gate
SHALL не ослабляться для принятия оптимизации.

Код: [coordinator](../../../../../crates/localvox-light/src/autocook.rs),
[cook](../../../../../crates/localvox-light-core/src/cook.rs),
[supervisor](../../../../../crates/localvox-light-process/src/lib.rs).
Проверки: [coverage](../../../../evidence/coverage.md).

#### Scenario: Следующая порция и Stop
- **WHEN** несколько finite horizons и final одной записи приходят без истечения keep-alive
- **THEN** engine загружается один раз для одной identity, готовые окна не распознаются снова, final текст/покрытие совпадают с one-shot.

#### Scenario: Смена модели или отказ
- **WHEN** identity изменилась, worker не отвечает/повредил protocol, обработка отменена или daemon остановлен
- **THEN** чужой engine/ответ не принимается, процесс освобождается, старые валидные результаты остаются доступными для recovery без fake success.

#### Scenario: Простой и управление архивом
- **WHEN** между заданиями истёк keep-alive либо выбран 0, а пользователь меняет/удаляет освобождённую запись
- **THEN** модель не удерживает память после срока, worker не держит session locks; правила mutation/cancel действуют как раньше.

#### Scenario: Повреждённый кеш
- **WHEN** saved result/checkpoint/PCM повреждён между запросами или во время recovery
- **THEN** повреждённая работа не продвигает подтверждённую границу; повторяются только необходимые окна, публикация остаётся атомарной.
