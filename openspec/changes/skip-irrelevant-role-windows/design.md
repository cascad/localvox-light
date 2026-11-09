# Design

## Context

См. proposal.md. Role entities используются только в сравнении пересекающихся
name/role spans. Decoder строит любой entity строго из слов текущего окна; NMS
локален окну, глобальный шаг только удаляет дубли. Это позволяет исключить окна,
из которых ни один role entity не способен пересечь name.

## Goals / Non-Goals

Уменьшить inference при точной сохранности результата успешной проверки.
Не менять name pass, labels/thresholds, разбиение слов, geometry/context окон,
decoder/NMS/dedup и сравнение scores. Не вводить эвристики по словам/языку,
приблизительные допуски, постоянный кеш, runtime fallback или смену модели.

## Decisions

- Общий iterator прежних окон 120 words / overlap 20 для full/relevant scans.
  Выбор касается целого окна, не обрезает/пересобирает его; model input оставшегося
  окна в точности прежний. Общий collector вызывает inference и propagates error.
- Индекс объединённых отсортированных half-open byte intervals имён. Пересечение
  ищется binary search; не полагаться на порядок/непересечение исходного NER output
  и не вводить квадратичный поиск names на каждом окне. Индекс не содержит текста.
- Доказательство: role span целиком внутри своего окна. Если он пересекает name,
  окно тоже пересекает name и будет вычислено. Все влияющие role spans сохраняются
  вместе с исходным контекстом/NMS; удаление остальных не меняет dedup/scores.
  Границы, касающиеся без overlap, не влияют на строгое сравнение в текущем коде.
- Успех не требует ненужного inference. Пропущенные операции не запускаются;
  ошибки необходимых окон и name pass по-прежнему возвращаются без маскировки.
  Full scan доступен только как явный engineering reference в profile module,
  на той же реализации. Он не служит скрытым runtime fallback.
- В profile добавить windows_skipped; skipped не выдаётся за completed/inference.
  Ordinary API сохраняет выключенный observer. Публичные пользовательские действия
  и settings/default models не меняются; справочник не перегружать инженерными flags.
- Model-free tests проверяют Unicode/half-open границы, nested/unsorted/adjacent
  spans, пустой и плотный набор имён, полноту сохраняемых пересечений, исполнение
  прежних окон и ошибки только необходимых вызовов. Native synthetic full/relevant
  equality включает scores/byte spans, overlap и thresholds 0.3/0.5.
- Два fresh release сравнения полной публичной source STT с обратным порядком
  full/relevant вызовов: exact Entity equality, времена/counters, без текста в
  отчёте. Затем два обычных grounding profile прежней STT/summary, проверка verdict,
  document cache и fingerprints. Timeout 300 s на процесс, без LLM/STT/download,
  сборки заканчиваются до замеров. Модель/источники/worker не менять.

## Risks / Trade-offs

Около каждого name нужны все пересекающие окна, включая соседние → геометрическое
покрытие и exact native tests. Модель с плотными names может не дать ускорения →
не ухудшать результат ради цифры, показать фактические skipped/calls. Состояние
дискового кеша/desktop влияет на wall → обратный порядок, отдельные preparation
и inference times, без общего SLA. При необходимости хрупких исключений остановить
внедрение и обсудить с владельцем; не подгонять контракт/тесты под дефект.

## Migration Plan

Изменение исходников без deployment рабочего daemon/worker. После tests/native
parity и измеренного эффекта обновить coverage/docs/evidence, закоммитить этап.
Mac/аппаратная приёмка и общая точность summary остаются not-run.
