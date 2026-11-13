# Design

## Context

См. proposal.md. Внешний тест копирует libtest helper во временный каталог,
поднимающий HTTP, настоящий TUI через ConPTY и Chromium. `ManagedProcess`
глушит stdout/stderr, а внешний тест читает только sentinel. Hosted artifacts
показывают прогресс обоих клиентов без panic hook, поэтому причина пока неизвестна.

## Goals / Non-Goals

**Goals:** воспроизведение и диагностика всего owned process tree, сохранение
проверок API/archive, чтения, заметок, настроек, reconnect и client-only exit.

**Non-Goals:** изменение контрактов продукта, запуск моделей/записи, обновление
визуальных baseline, пропуск проблемного шага или увеличение timeout без причины.

## Decisions

- Использовать существующий bounded supervisor с файловыми stdout/stderr для
  конечного helper, если подтвердится потеря диагностики. Это сохраняет ownership
  и timeout; прямой неуправляемый spawn оставил бы потомков при сбое.
- Определять успех по exit status и sentinel: один sentinel скрывает native abort.
- Исправлять источник воспроизводимого сбоя, сохраняя реальные UI/TUI и HTTP.
- Native reproduction на CPAL 0.17.3: первый std thread перечисляет устройства,
  завершается, второй получает 0xC0000005. Это известный upstream #1302.
  Обновление до 0.18.2 не исправляет apartment lifetime (проверен release source).
- Общий audio host на Windows создаёт MTA перед обращением к CPAL и сохраняет
  MTA support на время process-wide CPAL enumerator через официальный
  CoIncrementMTAUsage. Thread-local guard балансирует CoInitializeEx/CoUninitialize.
  Только успешный init кешируется; STA-конфликт возвращает ошибку, не используется
  как разрешение вызвать CPAL с несовместимым apartment.
- Process-wide MTA guard имеет тот же срок жизни, что static enumerator CPAL;
  не освобождается посреди процесса и не создаёт parked threads. Cookie —
  документированный cross-thread token; указатели COM объектов мы не переносим.
  Один retained MTA support на процесс, cleanup ресурсов выполняет ОС при выходе.
- Все Windows CPAL host entrances идут через этот adapter, включая watcher и
  capture restart; test-only initialization или подмена списков скрыли бы дефект.
- Native regression запускается в собственном supervised процессе: несколько
  закончившихся потоков discovery и параллельные запросы; устройства не открываются.

## Risks / Trade-offs

- Windows native abort может не вызывать panic hook → сохранять stderr и status.
- Тест зависит от готовых UI/TUI/Chromium → проверять их наличие, без подмены mocks.
- Локальный green не доказывает hosted green → отдельные ссылки и статусы evidence.
