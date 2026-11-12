# Design

## Context

См. proposal. Queue уже имеет commit_jobs с rollback изменённых Job, но часть
старых мутаций обходит его. У HTTP есть RAII для долгих LLM requests, однако
общий inflight вручную decrement-ится в конце spawn closure.

## Goals / Non-Goals

Сохранить JSON/wire, eligible states, phase/force/follow_settings и budgets.
Не менять policies enqueue/retry или строить новую state machine/framework.
Не держать полную копию очереди на каждую фазу и не удваивать запись на recook.

## Decisions

- update_job получает индекс/название перехода/closure для одного Job; clone
  только изменяемого Job и existing commit_jobs обеспечивают rollback.
  Existing result/void wrappers сохраняются. Различия reset полей явные.
- Удаления используют общий retain с rollback списка только для фактического
  удаления. history trimming при load не меняется. New job сохраняется один раз
  с полным request; failure не расходует next_id и не оставляет частичный job.
- RAII request budget использует atomic bounded acquisition и владеет permit
  через spawn closure. Drop при отказе spawn/панике/return возвращает permit;
  ручной decrement удаляется. Отдельный budget на route/ask остаётся 4.
- Tests инъецируют storage failure существующим deny_queue_target, сравнивают
  весь JSON до/после и retry; spawn failure — через narrow spawn seam с owned
  closure, без реального исчерпания OS threads. Unwind проверяется настоящим
  thread panic. Настоящий tiny_http overload проверяет 503 и восстановление.

## Risks / Trade-offs

- Случайное объединение разных retry policies → существующие queue tests и новые
  field/eligibility tests; public signatures не меняются.
- Clone всей очереди на горячем пути → копируется один Job; bulk removal отдельно.
- Permit утечёт до/после spawn → capture before spawn + dropped closure/error tests.
- Panic не ловится ради фиктивного ответа; RAII только возвращает slot, сервер
  обслуживает остальные запросы. Process abort/SIGKILL не объявляется unwind.

## Migration Plan

Queue fault reproduction → checked updates → regressions и первый commit;
HTTP RAII → failure/overload tests → финальная общая проверка, evidence и commit.
Пользовательский daemon/archive не запускать и не останавливать.
