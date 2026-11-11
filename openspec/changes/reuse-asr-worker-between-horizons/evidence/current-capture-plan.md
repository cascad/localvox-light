# Дополнительный контроль при одинаковом capture · до замера, 2026-11-11

Это дополнительный диагностический comparator, не замена исходного R4 и не
разрешение включить default on. Исходная серия с legacy offline заканчивается
и сохраняется со своим результатом; её порог/порядок/артефакты не меняются.

## Причина

В [рабочем pipeline](../../../../crates/localvox-light-core/src/pipeline.rs)
`ensure_current` создаёт capture Writer и attaches planner без проверки настройки
раннего ASR; `feed` использует `ChunkRecorder::for_capture` при обоих режимах.
Прежний [profiler](../../../../crates/localvox-light-bench/src/bin/localvox-incremental-profile.rs)
создавал capture/planner только для I; O использовал legacy `ChunkRecorder::new`.
Таким образом, исходное сравнение включает цену изменения первичного сохранения,
планирования и cache-enabled final, а не только запуска ASR до Stop. Это вывод
из различия входных путей; вклад wall ещё не установлен измерением.

## Зафиксированное сравнение

- Тот же целый публичный PCM 114344438 samples / 7146.527375 s; input SHA
  `96e50ce45af3988b3b069c0ebd4363842eb06b3fe5b536490a8dc9d81f02cd46`.
  Источник 1 — actual zero PCM, оба источника до настоящего Stop.
- Те же GigaAM int8/vocab SHA и recipe r7, 15/8/500, язык ru, diarization off.
  Один и тот же release worker для O/I; SHA входа/worker/dependencies до/после.
- `--current-capture-baseline`: обе стороны используют нынешние capture/planner;
  O запускает только full one-shot ASR после Stop; I использует resident prefix/final.
  Во всех O/I свой новый session/cache. Стандартный CLI без флага сохраняет legacy O.
- Три alternating пары O1/I1 steady300, O2/I2 burst900, O3/I3 steady300.
  Те же active wall = producer + workers, окна/PCM SHA/source/time/text/calls,
  отдельные CPU/logical I/O/high-water/startups/loads/Stop timings.
- Для дополнительного контроля тоже **≤20% в каждой паре и суммарно**. Порог
  не подбирается по данным. Все неуспешные пары сохраняются; exit0 не означает pass.
- Сначала short 330 s / 1 pair harness probe, затем целая серия в новых ignored
  marked outputs. Сборки/другие ASR не идут одновременно; системные caches/службы
  и личный архив не меняются. Изменений production pipeline для этого сравнения нет.

Даже успешный дополнительный контроль не переписывает исторический R4, не закрывает
аппаратные gates и сам по себе не меняет PIPE-018/default. Решение о comparator
для будущего принятия — отдельное от измеренного результата.
