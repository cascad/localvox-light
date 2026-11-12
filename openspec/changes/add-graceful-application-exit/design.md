# Design

## Context

См. proposal. Daemon уже имеет общий bounded shutdown для tray/signal/headless:
autocook, engine flush, ask-worker, voice. HTTP живёт в отдельном потоке; простой
ответ «accepted» не доказывает сохранение записи и может потеряться при process exit.

## Goals / Non-Goals

Получить результат существующего shutdown через API и дать HTTP закончить ответ.
Не вводить второй lifecycle, kill по PID/имени, ожидание часового LLM или авто-retry.

## Decisions

- API ApplicationControl разделяет running с daemon и хранит единственный
  запрос/результат cleanup под mutex/condvar. Работа выполняется прежним owner.
- POST /api/application/shutdown с confirm=true обрабатывается после обычной
  авторизации/origin/body checks. API без runtime control возвращает 501.
  Повторный stop и новые mutations во время остановки отклоняются.
- HTTP ждёт cleanup до 60 секунд. Daemon публикует результат после всех компонентов
  и даёт до 2 секунд на отправку ответа через RAII request guard; обычная остановка
  без запроса не получает этой задержки. Альтернатива «порт исчез = успех» отвергнута:
  crash/ошибка flush неотличимы от успешного завершения.
- Independent client выполняет один POST с timeout 75 секунд, проверяет отдельный
  completed=true. TUI control lane/отдельный resource: форма с адресом и ДА,
  видимое ожидание; смена подключения и новые действия до ответа заблокированы,
  фоновый polling приостановлен. Ошибка остаётся в общей консоли, клиент не закрыт.
- Обычные qq/F10/Ctrl+Q сохраняют client-only поведение. Палитра даёт редкому
  полному выходу явное имя без ещё одного скрытого сочетания.

## Risks / Trade-offs

- Потеря ответа после остановки → честное неопределённое состояние в клиенте,
  без автоматического повторного POST/bootstrap.
- Блокирующий компонент → прежние bounded joins и их диагностика; успешный ответ
  не маскирует engine/autocook failure. Interrupted jobs восстанавливаются как раньше.
- Настоящий микрофон/Mac → ручная приёмка отдельно; tests используют isolated
  HTTP/owner fixtures и ConPTY, не пользовательский демон/архив.
