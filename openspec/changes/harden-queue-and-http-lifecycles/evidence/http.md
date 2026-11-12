# HTTP request ownership · 2026-11-12

## Code-reviewed

API-003 сохраняет MAX_INFLIGHT=32, MAX_ROUTE_INFLIGHT=4, тело 1 MiB и прежний
ответ 503 при общей занятости. Auth/Host/Origin/media/route/ask logic не менялась.

Раньше общий счётчик decrement-ился после handle_request и не освобождался при
thread panic или отказе Builder::spawn. Теперь RequestBudget получает bounded
atomic permit; request closure владеет им до выполнения или drop. Permit::run
сохраняет guard на время обработчика; unwind возвращает slot. Thread start error
логируется, захваченный request/permit освобождается. tiny_http сохраняет своё
прежнее поведение unhandled request (500).

`serve_with` — private narrow spawn seam. Production serve использует настоящий
Builder::spawn и прежние detached request threads. Нового пула/настройки/async
runtime/зависимости нет. Existing LLM RouteGuard не менялся.

## Test-run

`cargo test --release --locked --offline --jobs 2 -p localvox-light-api --lib
budget`: **5 passed / 0 failed**.

- Настоящий worker thread паникует внутри Permit::run; join получает error,
  следующий request может получить slot.
- Owned task удалён до выполнения; slot возвращается. Normal return тоже
  возвращает slot.
- 16 конкурирующих попыток при limit=4: ровно 4 держателя, остальные получают
  отказ. После release доступна вся ёмкость; Condvar, без spin/sleep heuristics.
- Настоящий loopback HTTP + injected spawner: 32 последовательных thread start
  errors, прежние 500; следующий настоящий spawned health handler отвечает 200.
  OS threads не исчерпываются ради теста.
- Настоящий tiny_http serve loop: 32 queued owned closures/соединения удерживаются
  spawn seam; следующий HTTP получает 503. После выполнения одного handler новый
  запрос health отвечает 200. Сервер/сокеты/closure имеют cleanup в test fixture.

Полный API/regression и release/package checks — [финальный прогон](final.md).
Эти tests не имитируют process abort/SIGKILL и не являются сетевой/аппаратной
приёмкой Mac. Пользовательский daemon/архив/модели не использовались.
