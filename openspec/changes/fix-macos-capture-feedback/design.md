# Design

## Context

См. proposal. CPAL 0.17.3 создаёт CoreAudio process tap только для output-only
device; duplex device обрабатывает как input. Usage descriptions отсутствуют
в standalone daemon. Daemon передаёт None в UiMsg, capture errors остаются
в stderr. /api/record уже регулярно читается обоими клиентами.

## Goals / Non-Goals

**Goals:** честная обратная связь capture, штатный consent macOS, сохранение
работающего микрофона при проблеме системного источника.

**Non-Goals:** новый backend вместо CPAL, частные TCC API, выдача разрешений
за пользователя, заключение о разрешении по нулевым PCM, смена выбранного
устройства или моделей. Поддержка duplex tap требует отдельной реализации;
в этом исправлении такой output получает явную ошибку, а не ложный sys.

## Decisions

- Встраивать Info.plist в Mach-O daemon при линковке, включая direct cargo builds.
  Это покрывает terminal launcher без обязательной .app обёртки. Проверять реальную
  секцию собранного macOS binary в native CI, а не только наличие XML в исходниках.
- Один CaptureMonitor принадлежит engine с существующим workspace lease.
  Он атомарно публикует состояния обоих источников только при переходах;
  API читает их только при живом engine. Новый запуск сбрасывает старые состояния.
- Callback не пишет файлы и не берёт mutex: только atomic флаг первого сигнала.
  Основной capture loop обновляет память; существующий device-health worker
  публикует изменения атомарно, без отдельного опроса/потока и без fsync в WASAPI
  packet reader. Состояние signal означает, что сигнал
  поступал с момента открытия потока; текущий уровень/запись на диск не обещаются.
- Ошибка callback завершает попытку потока и попадает в обычный retry/error путь.
  Открытие повторной неудачной попытки не стирает видимую ошибку; успешное открытие
  переводит её в ожидание сигнала. Retry не должен создавать новую ошибку в консоли
  при каждом HTTP poll. Повторное открытие без звукового сигнала ещё не закрывает
  серию одинаковых ошибок; подтверждение сигнала/выключение источника закрывает её.
- В /api/record добавляется обратно совместимое поле capture_sources. Тишина
  имеет спокойный статус/подсказку, actual errors идут в существующую консоль.
- Для macOS output, который CPAL открыл бы как microphone, возвращать явный отказ.
  Микрофон работает независимо; скрытый fallback на другую звуковую дорожку запрещён.

## Risks / Trade-offs

- Открытый CoreAudio tap может возвращать нули при отсутствии доступа. Поэтому
  описание доступа и ожидание сигнала обязательны; вывод PermissionDenied по одной
  тишине был бы неверным. Новый native Mac consent и звук требуют аппаратного повтора.
- Unsigned release/разные terminal hosts могут иначе атрибутироваться macOS TCC;
  фиксировать фактический результат и не объявлять разрешение выданным по build test.
- Состояние на диске переживает crash → API обязательно проверяет engine lease;
  новая engine instance инициализирует файл до открытия capture streams.
- Windows/Linux не меняют выбор устройств; регресс должен покрыть их capture
  lifecycle, quiet/disabled states и backward compatibility отсутствующего поля.

## References

- [Apple: CoreAudio taps и NSAudioCaptureUsageDescription](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps).
- [CPAL: известный silent-permission failure и пока не принятые helpers](https://github.com/RustAudio/cpal/pull/1257).
