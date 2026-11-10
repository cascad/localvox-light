# Ранняя расшифровка: snapshots и API

Поведение задаёт [change](../openspec/changes/add-incremental-recording-transcription/proposal.md),
main requirements пока не синхронизированы. [Пользовательские действия](user-guide.md#запись-и-обработка).

`capture/state.json` подтверждает закрытый PCM, `capture/plan/` — непрерывные VAD
окна. Worker получает checksum-named конечный plan, сохраняет проверенные ASR
results/checkpoint отдельно от versions/best. Coordinator освобождает STT линию
после порции и ждёт новых окон. Stop переводит то же задание в обычную финализацию.
Cancel — сохранённый запрет продолжения; Retry снимает его явно. Выключение раннего
режима завершает текущую порцию и оставляет обычную обработку после Stop.

## Ответы и границы

- `GET /api/sessions/{name}/transcript` и `/processed`: до полного transcript receipt
  возвращают один partial snapshot, `partial` и строки со стабильным `id`
  generation/source/start-end. `/processed` отдаёт исходный STT (`from_transcript`).
  После подтверждения полного результата `partial` отсутствует, обычные версии
  и правки работают как прежде. Неподтверждённый best не выдаётся за полный STT.
- `partial`: `generation`, `revision`, `snapshot`, `sealed_end_sec`, `stt_end_sec`,
  `text_end_sec`, `audio_end_sec`. Ревизия относится к сохранённому снимку, не к
  длине будущей записи. Нулевые границы до первого снимка означают ожидание.
- `GET /playback`: `duration_sec`, `snapshot`, `partial`. До подтверждённого полного
  STT аудио доступно только как `/audio.wav?snapshot=<SHA>`. Token/auth правила общие.
- `/audio.wav?snapshot=...`: неизменные WAV header/length и Range. Старый URL остаётся
  доступен после роста и Stop. Запрос за границу отклоняется. Первая порция PCM
  проверяется до успешных HTTP headers; ошибка позднее в потоке прерывает тело.
- `/peaks?buckets=...&snapshot=<SHA>`: та же длина/PCM. Нет подстановки тишины при
  повреждении. Обычный полный export недоступен до подтверждённого full transcript.
- `/progress` дополнен `partial`: сохранённое аудио/STT/публикация различаются.
  `waiting_input` означает отсутствие работающего worker, не Done всей записи.

Snapshot хранит checkpoint, plan, primary ranges и опубликованные строки целиком.
Сначала immutable file/results receipt, последним current pointer. Временный отказ
новой ревизии не портит старые URL. Чтение проверяет SHA и generation. PCM проверяется
по полным нужным VAD окнам; публикация консервативно задерживает строки, которым ещё
нельзя предоставить целое проверенное аудио. Короткая дорожка дополняется нулями
только после её явного EOF. Неизвестная/не поступающая дорожка может задерживать
общую границу до Stop; отсутствие пакетов не является доказательством отключения.

## Память и клиенты

Один HTTP ответ держит не более одного container reader и одного PCM окна на источник,
плюс буфер выдачи 1 MiB. Shared chunk lease удерживается через чтение; FLAC декодируется
один раз на контейнер/ответ в bounded temporary file на диске аудио. Полное аудио
или новая копия архива на каждую ревизию не создаются.

UI/TUI отвергают ответы старой ревизии/поколения и late partial после final. Ключи
строк удерживают выбор; raw Текст в браузере дополняется неизменными inline spans,
чтобы не заменять текстовый узел под выделением. Копирование включает весь текущий
доступный текст. Playing snapshot и waveform закреплены до действия пользователя:
в UI — кнопка `+ время` или переход к новой реплике, в TUI — `p`/переход к новому времени.

Результаты проверок: [R3](../openspec/changes/add-incremental-recording-transcription/evidence/partial-clients.md).
Аппаратный звук, длительный microphone/loopback и слабый Mac требуют отдельной приёмки.
