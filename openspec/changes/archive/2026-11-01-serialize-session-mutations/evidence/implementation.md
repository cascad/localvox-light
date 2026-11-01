# BL-002: отмена и согласованное владение — 2026-11-01

Реализация и проверки на Windows, текущая debug-сборка. Использовались только
временные архивы в `target/test-temp` на D:, синтетическая расшифровка и loopback
HTTP-стенд. Установленный daemon, пользовательский архив, устройства и внешняя
LLM не перезапускались. Это проверка исполнения/сохранности, не качества моделей.

## Поведение и входы

- [Core cancellation](../../../../../crates/localvox-light-core/src/jobs/cancellation.rs):
  сохранённые `Cancelling`/`Cancelled`, fence для claim/advance/failure,
  восстановление лишь при свободном реальном lease, ручной retry той же фазы.
- [Владение](../../../../../crates/localvox-light-core/src/jobs/control.rs):
  `.mutation.lock` → `.worker.lock`, registry canonical path + job/execution,
  независимый stop token; никакой остановки по произвольному PID.
- [Координатор](../../../../../crates/localvox-light/src/autocook.rs): регистрация под
  claim mutex, supervisor с per-session stop, исход сохраняется до освобождения lease.
- [API](../../../../../crates/localvox-light-api/src/archive/mutations.rs): POST cancel,
  delete после подтверждения владения, отказ до записи для остальных мутаций;
  запись аудио и подтверждение удаления проверяются перед сигналом.
- [UI](../../../../../ui/src/Queue.tsx) и [TUI](../../../../../crates/localvox-light-terminal/src/app/commands.rs):
  остановка/отмена отдельно от ошибки/Done; продолжение вручную, `X` и `R` вне форм.
  [Сценарии US-008/023/047](../../../../../docs/user-guide.md).

## Каждый сценарий delta

| Сценарий INV-SESSION-001 | Выполнено и результат |
|---|---|
| Delete during worker execution | Настоящий `localvox-process --worker-phase summary` дошёл до удерживаемого локального HTTP-запроса. API прекратил принадлежащее дерево, дождался освобождения lease и лишь затем удалил каталог/очередь. Владелец перед освобождением проверил исходный текст побайтово. |
| Stop fails before deletion | Удерживаемый managed lease дал timeout: каталог остался, позднее освобождение/maintenance только подтвердило отмену. Windows deny-replace до сигнала и при подтверждении остановки не позволил удалить файлы. При отказе физического удаления оставшийся файл и Cancelled fence сохранены, повтор удаления после снятия отказа успешен. |
| Cancel after a completed phase | Настоящий worker остановлен во время HTTP: проверенная STT сохранилась побайтово, summary не стала Ready; operation_end имеет cancelled. Core load/recovery/revival/claim не возвращают отмену в Pending; настоящий coordinator запущен с сохранённым Cancelled и не сделал ни одного claim. Явный retry возвращает ту же фазу. |
| Change language during worker execution | Прежняя проверка всех API-мутаций при живом `.worker.lock` повторно пройдена с побайтовым сохранением meta и производных. |
| Change best or names during worker execution | Та же проверка подтвердила отказ best/source/speaker без скрытой записи; control lease также исключает параллельную API-мутацию. |
| Manual CLI competes with API | Настоящий standalone `localvox-process --post-only --summary` удерживал тот же OS lease и ждал local HTTP. Cancel/delete отказали с объяснением CLI, jobs.json и исходная версия не изменились. Тестовый supervisor затем остановил только свой CLI и подтвердил освобождение. |
| Mutation uses an alias instead of the archive identifier | На Windows запрос FIXTURE вместо точного fixture отклонён до записи; не создан второй tombstone. Код проверяет canonical parent и точное имя для всех API-мутаций. |

## Выполненные проверки

Команды из корня, `cargo ... --locked -j 2`, Rust tests с `--test-threads=2`,
TEMP/TMP на D:. Указаны фактические прогоны; ignored не считаются выполненными.

| Набор | Результат |
|---|---|
| Core jobs / workflow / journal / progress | 69 / 11 / 16 / 10 passed; 1 progress fixture ignored |
| API library | 75 passed, 6 ignored; включает 9 mutation/fault и 2 новых настоящих HTTP/client сценария |
| Native worker + standalone CLI | 2 passed, отдельно `--ignored autocook::cancellation_tests`; worker test выполняет cancel и delete |
| Coordinator library | 14 passed, 3 ignored; 2 native теста выше запущены отдельно |
| Rust client / transport | 3 + 8 passed |
| Process ownership / ConPTY / responsiveness / run-script | 12 / 2 / 2 / 2 passed; 1 process fixture ignored |
| TUI library | 98 passed, 2 ignored; включает X/dedup/ручной R и новый snapshot отменённого журнала 80×24 с остановленными часами |
| UI | build и 10 unit passed; lint без ошибок, 8 существующих warnings. 73 browser e2e passed, включая 5 новых сценариев отмены и 3 ожидаемых отказа visual detector. Новый тест документа отдельно прошёл 3 повтора после исправления ожидания загрузки |
| Оба клиента → один настоящий временный API | shared_workspace: 1 passed, отдельно ignored; не аппаратный звук |
| Profiler / queue runner | 21 + 4 passed, по 1 ignored; native coordinator lifecycle отдельно 1 passed, включая Cancelled без повторного захвата |
| Launcher / HTTP journal contract | 1 + 1 passed |
| Headless / worker / TUI / queue runner | no-default-features check и сборка native binaries прошли |

Визуальные эталоны: новая очередь cancelled на 480px, новый TUI cancellation
snapshot; в трёх прежних browser snapshots обновлено намеренное размещение кнопки
отмены. Изображения и текстовый snapshot просмотрены; остальные эталоны сохранены.

В queue profiler исправлено имя `LOCALVOX_LLM_GLOSSARY_DIR`, отмена считается
терминальным неуспешным исходом вместо бесконечного ожидания. Предыдущие измерения
не пересчитывались: их fresh current_dir уже задавал пустой каталог glossary.

## Границы

- Один владелец jobs.json/daemon. Registry внутри процесса не даёт управления
  посторонним CLI или daemon в другом процессе. На других ОС этот набор не запускался.
- Подтверждается остановка локального дерева, не прекращение вычисления внешней LLM.
- Таймаут ожидания остановки в обработчике API не оставляет отложенного delete.
  Потерянный сетевой ответ сам по себе не доказывает отказ уже выполненной операции.
- После физического удаления ошибка очистки очереди оставляет безопасный Cancelled
  tombstone; после частичного remove_dir_all нет отката всего каталога.
  Lock не создаёт многофайловую crash-транзакцию. Проверены конкретные Windows
  deny-replace/deny-delete и restart состояния, не внезапное отключение питания.
- Аппаратная приёмка UI/TUI: микрофон, системный звук, слышимый seek/fragment,
  выход TUI во время записи и исходный release-инцидент зависания остаются not-run.
  Эта работа закрывает BL-002, а не остальные RV/BL или BL-018 целиком.

Структура и ссылки: strict main specs 15/15, strict change valid;
`python scripts/check-openspec.py`: 15 capability, 87 требований, 175 сценариев,
87 строк coverage. Formatting API/изменённых Rust-файлов и `git diff --check` пройдены.

Delta синхронизирована в main specs по официальному skill, все 7 сценариев сверены.
Change архивирован CLI 1.13.2 с `--skip-specs` после sync (без повторного ADDED).
Единственная незакрытая на момент перемещения задача 1.4 была самой финализацией;
после успешного переноса отмечена выполненной. Реализация не архивировалась с пробелом.
