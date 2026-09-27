# API and remote access Specification

## Purpose

F6/F7. HTTP, доступ из браузера/телефона и MCP-сервер архива.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#api-access).

## Requirements

### Requirement: API-001 Обычная авторизация HTTP

Обычные API-маршруты SHALL принимать Authorization: Bearer при настроенном токене; без токена проверять loopback клиента и Host/Origin. Токен в query обычного /api/health не принимается. Содержимое тела читается после авторизации; неверный доступ получает 401/403.

Код: [handle_request, authorized, respond](../../../crates/localvox-light-api/src/http.rs).

#### Scenario: Query на health

- **WHEN** клиент передал только ?token при включённом токене
- **THEN** обычный endpoint отклоняет запрос.

#### Scenario: Чужой Host

- **WHEN** token отсутствует, Host не loopback
- **THEN** обычный обработчик возвращает 403.

### Requirement: API-002 Исключение для media URL

GET audio.wav SHALL поддерживать токен в query либо Bearer при заданном токене для native audio-элемента. Без токена SHALL применяться те же проверки loopback клиента и Host/Origin, что для обычного API. Исключение касается способа передачи токена, а не ослабления границы доступа.

Код: [handle_request, authorized, respond](../../../crates/localvox-light-api/src/http.rs), [audio URL](../../../ui/src/Player.tsx).

#### Scenario: Плеер с токеном

- **WHEN** audio.wav открыт с правильным query token
- **THEN** аудио доступно без Bearer-заголовка.

#### Scenario: Без токена

- **WHEN** локальный клиент без токена обращается к audio.wav с чужим Host или Origin
- **THEN** запрос отклоняется до чтения аудио, как запрос к обычному API.

### Requirement: API-003 Ограничение HTTP работы

HTTP serve SHALL ограничивать одновременные запросы MAX_INFLIGHT=32, тело MAX_BODY_BYTES=1 MiB и отдельные длительные route/ask через RouteGuard с лимитом 4. При общей занятости возвращается 503. Медленный LLM-запрос не намеренно выполняется в единственном accept-loop.

Код: [handle_request, authorized, respond](../../../crates/localvox-light-api/src/http.rs).

#### Scenario: Перегрузка

- **WHEN** общий лимит запросов достигнут
- **THEN** новый получает 503.

#### Scenario: Большое тело

- **WHEN** тело превысило 1 MiB
- **THEN** запрос отклоняется, даже если лимит текста asks в символах ещё не превышен.

### Requirement: API-004 MCP-сервер файлового архива

McpServer SHALL обслуживать JSON-RPC stdio: initialize, ping, tools/list, tools/call. Доступны search_transcripts, list_sessions, get_transcript, get_summary, append_note. Уведомления без id не требуют ответа; неизвестный метод/инструмент даёт протокольную ошибку. Для чтения архива работающий capture-engine не требуется.

Код: [McpServer::handle/dispatch](../../../crates/localvox-light-api/src/lib.rs).

#### Scenario: Notification

- **WHEN** сообщение не имеет request id
- **THEN** ответ не отправляется.

#### Scenario: Неизвестный tool

- **WHEN** tools/call указывает отсутствующее имя
- **THEN** возвращается ошибка вместо фиктивного успеха.

### Requirement: API-005 Один web UI для desktop и LAN

Демон SHALL отдавать тот же React frontend браузеру и Tauri; данные получаются через API. Доступ с телефона требует достижимого адреса демона и соответствующей авторизации. Оболочка UI не является отдельной копией архива; в маршрутах не найден реализованный WebSocket-контур событий из старого F6.

Код: [handle_request, authorized, respond](../../../crates/localvox-light-api/src/http.rs), [api, token](../../../ui/src/api.ts), [run](../../../src-tauri/src/lib.rs).

#### Scenario: Окно закрыто

- **WHEN** демон продолжает работать
- **THEN** страница может быть открыта в браузере по его адресу.

## Implementation status

API-002 нарушен: media без токена обходит Host/Origin (RV-003). Это воспроизведено на временном сервере до 404 отсутствующего аудио; не проверка выдачи настоящего файла. LAN/mobile/capture не приняты.

[Матрица по требованиям](../../evidence/coverage.md#api-access) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

API-002 требует ревью RV-003. Наличие PWA-манифеста не доказывает offline cache, native mobile capture или синхронизацию телефона. Полный сетевой прогон не выполнялся; `token_required_for_remote_and_checked` и `host_loopback_check_matches_only_local` в http.rs — связанные тесты обычных маршрутов, не доказательство всех исключений.
