# Design

## Context

См. proposal.md. `Ner.names` последовательно запускает name и role classifiers;
каждый `extract_at` делит исходник на слова и перекрывающиеся окна. На каждом
окне выполняются tokenizer, masks/tensors, mutex session, session.run, копирование
logits, декодирование и подавление пересечений. Затем удаляются оконные дубли и
сравниваются name/role scores. Общая стоимость этих действий пока неизвестна.

## Goals / Non-Goals

Получить exclusive численные интервалы и реальные counters окон/inference,
сохранив порядок вычислений и точные entities/score/spans. Не менять окна,
пороги, labels, provider/модель, threads, retries и состав пользовательских этапов.
Не вводить глобальный observer, постоянный кеш или новые события журнала.

## Decisions

- Общая реализация с локальным observer: штатные `names`/`extract_at` выключают
  его, opt-in `names_profiled` возвращает отдельно результат и численный профиль,
  в том числе при ошибке. Измерять только `session.run` недостаточно: нужны также
  подготовка и декодирование; ORT tracing не включается, чтобы не писать входы.
- Агрегировать по pass (name/role/comparison) и статической категории: split words,
  window input, tokenization, tensor preparation, session lock, tensor creation,
  inference, logits copy, span decode, window dedup, role comparison. Число вызовов,
  ошибок, начатых/завершённых окон, tokens total/max. Никаких texts/labels/entities/
  logits/error strings. Размер отчёта не зависит от числа окон.
- Сумма категорий exclusive, общий `names` wall включает instrumentation/остаток.
  В bench эти интервалы вложены в NER backend, который вложен в grounding request;
  не складывать их повторно. Ошибки отмечаются у категории и `complete=false`.
- Расширить существующий `localvox-grounding-profile` флагом `--ner-components`.
  Первый/повторный grounding записывают core profiles; перед обычной parity
  проверкой core observer выключается. Source cache остаётся document-local,
  lookup не подделывается под повторный inference. Без флага старый профиль доступен.
- Model-free проверки observer и probe: сохранение результата/ошибки и порядка,
  exclusive/finite times, численные partial/error, privacy, отсутствие ложных
  окон/проходов и отключение измерений при ordinary parity. Отдельная explicit
  native проверка сравнивает полные `Entity` (включая scores/byte spans) на короткой
  синтетической речи, пересечении окон, пустом тексте и отключённом role pass.
- Два последовательных свежих release процесса на прежней сохранённой публичной
  STT/summary, два profiled checks и один ordinary parity check в каждом. Timeout
  300 s на процесс, без LLM/STT/download. До/после совпадают source/answer/model SHA;
  во время измерений нет сборок. Файловый кеш ОС не очищается.

## Risks / Trade-offs

Инструментация может добавлять небольшую задержку → штатный путь без clocks,
замер residual и точная parity. CPU inference зависит от desktop-нагрузки → два
запуска и отсутствие общего SLA. Отказ до inference не равен пустому успешному
результату → partial/error counters, исходная ошибка возвращается вызывающему.
Сохранённый final markdown не воспроизводит промежуточные map/reduce ответы →
новые результаты относятся только к проверке готовой пары, качество summary не принято.

## Migration Plan

Без миграции и установки. Собрать отдельный release probe, сохранить numeric
evidence, обновить coverage/engineering docs/CI и коммит. Рабочий worker/daemon
и пользовательский архив не заменяются; оптимизация требует отдельного решения
по измеренным компонентам.
