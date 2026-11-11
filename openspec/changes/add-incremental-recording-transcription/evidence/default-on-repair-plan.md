# Исправление задержек перед default · план повторного контроля, 2026-11-11

Первый [полный current-capture прогон](default-on-native-r1.json) завершился fail:
+3.42% / +3.83% / +68.75%, суммарно +25.64%. Окна/текст/714 calls совпали.
В медленной порции recognize=4.017 s, cache=13.607 s. Владелец после получения
результатов выбрал «Сначала исправить задержки, затем включить».

Короткий тест настоящего result journal (1608 frames, 1 014 648 bytes, без ASR)
дал 19.005 s: write=0.042 s, sync=11.742 s, 1608 синхронизаций. Это подтверждает
цену отдельных sync; всё время native cache нельзя автоматически приписать одному
syscall. Для дальнейшего native измерения добавлены write/sync/max-sync counters.

## Исправление

- Общий ResultBatch для prefix и full cook: не более 8 outputs и 1 MiB пакета;
  большой одиночный record ограничен прежним 8 MiB. PCM в пакете не удерживается.
- Подготовленные outputs — только memory previews, не Accepted. WAL сохраняет
  прежние frames/checksums, синхронизирует пакет, затем результаты перечитываются.
  Только после этого выдаются Accepted и разрешены checkpoint/text/version/receipts.
- Ошибка write/sync не принимает группу. Complete orphan frames после отказа
  sync перед повторным использованием дополнительно синхронизируются один раз
  на segment/operation; прежние атомарные JSON записи остаются совместимыми.
- Crash до группового commit допускает повтор только неподтверждённой работы.
  after_result fixture теперь обозначает durable group; отдельный after_pending_result
  проверяет реальный обрыв до sync. Специального обхода алгоритма для fixtures нет.
- Не меняются PCM windows, model/recipe/fresh SHA guards, runtime/lease/cancel,
  форматы результата и worker protocol; настройки/интерфейсы не получают batch knobs.

## Уже выполнено перед повтором

19 incremental unit passed, 6 настоящих worker fixtures passed. В числе проверок:
pending/drop/duplicate, отказ записи, отказ sync/recovery sync, bounded group,
checkpoint/final crashes и совместимость legacy/force. После правки в одном
сопоставимом journal probe: single=6.471 s / sync=1.368 s / 1608 flush;
batch8=0.919 s / sync=0.185 s / 201 flush. Bytes и число frames одинаковы.

Прежний probe 19 s и новый single 6.47 s — разные состояния диска, не чистый A/B
эффект кода. В начале первой серии D: имел около 9.7 GB свободно, после неё 4.9 GB;
позднее стало около 216 GB. Наш сохранённый прогон занимает 2.787 GB и перенесён
в `F:\localvox-benchmarks\incremental-default-on-reference-20261111`, SHA отчёта
`282aa35de6691a984d8ba88fc5caede8d0e8ac20585d65af61dff238c26e2471` совпал.
Остальное изменение свободного места не приписывается этой работе.

## Фиксированный повтор до замера

На том же полном PCM/weights/recipe и одном новом release-worker, без --limit-sec:
сначала **3 alternating legacy пары** исходного gate, затем **3 alternating
current-capture пары**. Оба comparator сохраняют свой baseline и исходный порог
**≤20% по каждой паре и суммарно**. Нужны exact windows/text/calls, один engine
load/start на режим, отсутствие повторного ASR принятых окон после Stop.
Оба прогона остаются на D:, каждый в новом root/cache; результаты всех попыток
сохраняются. SHA до/после, отдельные CPU/I/O/RAM/wall/Stop timings обязательны.
Сборки и другой наш ASR во время серии не запускаются.

Default on, API/UI/TUI regressions и новый комплект — только после успешного
повтора. 7.4 hardware/Mac остаётся not-run, sync/archive/push не выполняются.

## Завершающий аудит до R3

Legacy R2 прошёл три пары на первом grouped candidate. Current R2 остановлен
до завершения из-за найденного при аудите случая: после порчи ранее принятой
записи её исправленный orphan может совпасть с прежним checkpoint, хотя sync
исправления завершился отказом. Восстановление checkpoint теперь также подтверждает
sync сегмента до принятия ссылки. Позитивное совпадение digest само по себе не
заменяет подтверждение записи. Новый тест воспроизводит этот случай; всего
**20 incremental tests и 6 actual worker fixtures passed** после исправления.

R2 не объявляется окончательным gate для изменённого recovery. В R3 повторяются
обе полные трёхпарные серии на одном финальном worker: legacy, затем current.
Новых изменений поведения ASR/storage после начала этих серий не планируется.
