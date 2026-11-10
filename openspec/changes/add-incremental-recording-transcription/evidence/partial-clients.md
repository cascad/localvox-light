# R3 · Готовая часть в UI/TUI, 2026-11-10

Реализованы partial Реплики/Текст, полный copy доступного документа, границы
аудио/STT/текста и waiting_input в журнале/очереди. Нет общего full export до
transcript receipt. [API/ownership и ограничения](../../../../docs/incremental-recording.md),
[короткие пользовательские действия](../../../../docs/user-guide.md).

Код: [snapshot](../../../../crates/localvox-light-core/src/incremental/publication.rs),
[PCM reader](../../../../crates/localvox-light-core/src/incremental/media.rs),
[API](../../../../crates/localvox-light-api/src/archive/partial.rs),
[media](../../../../crates/localvox-light-api/src/archive/media.rs),
[UI](../../../../ui/src/DocumentPane.tsx), [player](../../../../ui/src/Player.tsx),
[TUI](../../../../crates/localvox-light-terminal/src/app.rs),
[mpv](../../../../crates/localvox-light-terminal/src/player.rs).

## Выполнено

- Настоящий temporary HTTP: capture recorder + native VAD + synthetic cache results,
  рост 43+20 s, stable line IDs/порядок, одинаковые Реплики/Текст, запрет export и
  mutable capture audio, SHA-addressed старый WAV/Range/peaks. PCM диапазона 42–44 s
  точно совпал с producer через шов файлов. Corrupt нужного PCM даёт ошибку, старый
  независимый prefix играет; path-like snapshot отвергнут. **1 passed**.
- Настоящий FFmpeg: открытый HTTP reader удерживает WAV, converter bounded отказывает;
  после освобождения WAV→FLAC, тот же snapshot даёт побайтно то же аудио и waveform.
  Настоящий mpv + temporary HTTP + null audio: старый snapshot продолжает играть,
  пауза/граница не меняются при росте; явный Load новой реплики выбирает новый snapshot
  и заканчивается на заданной границе. **2 passed**, 12.38 s (последовательный прогон).
- UI build passed; unit **10 passed**; Playwright **77 passed**, 35.9 s, включая
  3 намеренных visual-negative сценария, которые ожидаемо отвергли symbol/color/geometry.
  Новый growing test проверил выделение и scroll через дополнение, late response,
  copy полного текста, неизменный playing URL/time/pause, явную смену снимка и final.
  Новый partial PNG просмотрен; прежние baselines не переписывались.
- TUI/client: terminal library **127 passed, 2 ignored**, client **11 passed**;
  обычные process/IO и ConPTY regressions passed. Новый ConPTY с настоящим бинарником
  увидел растущий префикс без смены строки старого фрагмента, восстановил терминал
  и оставил запись работать. Snapshot partial 120×36 просмотрен, принят и сравнивается.
  Unit проверяет stable anchor/scroll/playback, late generation/revision/final,
  полный copy и Cancel вместо Stop во время записи.
- API полный прогон: **80 passed, 8 ignored**. Аппаратные/real codecs/mpv ignored
  сценарии, выполненные выше отдельно, не выдаются за автоматически выполненные в suite.

Дополнительно `http::tests::cross_clients::shared_workspace --ignored --exact`:
**1 passed**, 4.87 s. Оба клиента работали с одним настоящим временным API/архивом.

Первый общий прогон выявил фиксацию обычной full-recording duration в UI player;
исправлено без reload source, соответствующие прежние browser regressions прошли.
DOM test full copy обновлён для неизменных inline span узлов вместо одного
перезаписываемого текстового узла. Первоначальный PCM error закрывал тело HTTP лишь
после клиентского timeout: первая bounded PCM порция теперь проверяется до headers,
тот же отказ завершается сразу; ошибки позднее в потоке по-прежнему обрывают тело.

В oversubscribed совместном Rust/browser прогоне core получил 447 passed/1 failed/
8 ignored: Windows atomic-replace test исчерпал 100 ms budget до повторного rename.
Результат не скрыт и budget не увеличен. Проверки повторены с CI-ограничением
`RUST_TEST_THREADS=2`: **448 passed, 8 ignored**, 10.68 s. Обычный real mpv/media
regression отдельно **1 passed**, 3.64 s. Изменений atomic writer
в этом этапе нет. Native model/reference performance — R4, hardware/Mac not-run.
