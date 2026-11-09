# Proposal

Статус: измерительный срез выполнен 2026-11-09; [результаты](evidence/results.md).
Ускорение/смена модели не выполнены и не считаются результатом измерения.

## Why

Продолжение BL-003: [профиль grounding 2026-11-03](../profile-summary-grounding-cost/evidence/results.md)
локализовал 54.632/61.785 s в первом source NER; подготовка/сопоставление стоят
сотые доли секунды. Для выбора ускорения нужно разделить person/role passes,
окна, tokenizer, подготовку tensor, session.run и декодирование.

## What Changes

- Opt-in численные timings и counters внутри той же реализации `Ner.names`;
  обычные API сохраняют прежние вычисления без чтения часов/новых логов.
- Дополнительный флаг существующего инженерного grounding profiler для записи
  агрегатов по name/role pass; без текста, найденных имён/spans и ошибок backend.
- Два ограниченных fresh release запуска на прежней публичной паре STT/summary,
  проверка exact parity, fingerprints и сохранности источников/моделей.
- Существенные model-free tests и отдельная явная native проверка совпадения
  entities/score/spans обычного и профилированного API. По цифрам определить
  следующий внутренний шаг; не менять модели/thresholds/windows/retries ради времени.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. Измерение реализации LLM-004/PIPE-007 без изменения контрактов;
`skip_specs: true`. Новая оптимизация или изменение проверки — отдельная работа.

## Impact

Core NER, существующий bench profiler, model-free CI, engineering docs и evidence.
Без новых зависимостей, LLM generation и пользовательских настроек. Рабочий
daemon/worker, архив владельца и установленные модели не заменяются.
