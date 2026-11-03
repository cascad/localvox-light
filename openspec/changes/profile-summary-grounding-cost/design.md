# Design

## Context

См. proposal.md. `check_all` объединяет детерминированную подготовку, requests к
NER, нормализацию/сопоставление и сборку результата. SourceEntities уже хранит
один точный plain-source в пределах документа. Сохранена публичная 704-строчная
STT и готовая сводка предыдущего native reference; промежуточные LLM ответы не
сохранялись. Поэтому новый замер — повторная проверка готового документа, не
воспроизведение всей генерации map/reduce.

## Goals / Non-Goals

Разделить реальную цену первой/повторных проверок без смены исходов/провайдера,
моделей, thresholds, prompts и политики retries. Не оптимизировать до измерения,
не вызывать LLM, не менять пользовательский журнал или его события, не читать
архив владельца. Полная приёмка смысла/качества и Mac остаются отдельными.

## Decisions

- Добавить opt-in численный профиль к общей реализации grounding. Штатные API
  используют ту же функцию с выключенным observer, без измерений/логов/кеша.
  Последовательные exclusive компоненты: source preparation, answer numbers,
  NER text preparation, source entity request/normalization, answer entity
  request/normalization, entity matching, lexical matching, result assembly.
  Total и время загрузки модели отдельно; вложенные времена не складывать дважды.
- Request к source включает существующий cache lookup; engineering adapter под
  SourceEntities отдельно считает фактические source/answer NER calls и их время.
  Первый и повторный вызовы различаются наблюдаемыми counters, не гипотезой о кеше.
  Имена, source/answer текст, spans и ошибки NER не сериализуются.
- Новый `localvox-grounding-profile` в bench принимает JSONL, готовый markdown,
  явный NER dir и новый output. Собирает speech из текстов реплик в порядке
  времени, использует встроенный summary template и пустой glossary, удаляет
  citation markers как pipeline. Никаких .env/активного пользовательского lexicon;
  Lexicon::builtin. Формат output численный, с SHA входов/бинарника/NER файлов.
- Два fresh release процесса: в каждом холодная по process residency загрузка NER,
  первая и повторная профилированные проверки одного документа, затем штатная
  непрофилированная проверка для parity по точному результату. Файловый кеш не
  очищать; NER не заменять/не квантовать. Сохранить все input hashes до/после.
- Source threshold 0.3, answer threshold и name/role labels как у текущего worker.
  Отказ NER учитывается численно: пустой Vec при ошибке не выдаётся за успех;
  профиль сохраняет incomplete/failure без текста ошибки. Output create_new,
  частичный отчёт доступен при остановке/отказе. Новые пути только в target на D:.
- Model-free tests проверяют parity с обоими штатными API, числа/NER/пустой ответ,
  cache hit/replacement, отсутствие приватных строк в JSON и отказ overwrite.
  Native GLiNER замер подтверждает стоимость/исходы именно сохранённой пары текстов.

## Risks / Trade-offs

CPU inference может занимать минуту → последовательные ограниченные прогоны,
численный progress и внешний timeout процесса. Профиль может чуть влиять на wall
→ те же вычисления без timings в обычном пути, parity и non-additive metadata.
Неизвестный bottleneck внутри ONNX → не приписывать всё время загрузке/токенизатору;
измеренные component boundaries и оставшийся unknown записать явно.

## Migration Plan

Нет миграции, установки и смены настроек. Инструмент/opt-in API и docs/evidence;
рабочий daemon/worker не заменяются. Изменения production reuse/NER — отдельная
работа после полученных результатов.
