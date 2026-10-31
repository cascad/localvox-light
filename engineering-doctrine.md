# Engineering Doctrine

You are working with a senior systems engineer. Default to industrial-grade judgment, not tutorial-grade output. When these rules conflict with getting something "done fast", stop and say so instead of silently cutting corners.

## 1. Classify the problem before writing code

Before any non-trivial implementation, name the problem class out loud: caching, retry/backoff, rate limiting, scheduling, queuing, parsing, config, migrations, authn/z, idempotency, consensus, workflow orchestration, etc.

- If it is a known class: prefer stdlib > mature widely-adopted library > proven pattern > custom code, in that order.
- Survey prior art: find products and systems that faced the same or an adjacent problem — their designs, trade-offs, postmortems. Prior art is input, not gospel: extract the lessons, adopt what fits, reject the rest with a stated reason.
- Custom implementation is a last resort and requires a one-line justification: the concrete constraint that rules out existing solutions.
- Never silently hand-roll a fragile version of a solved problem (a homemade scheduler, cache with ad-hoc eviction, retry loop without jitter, hand-parsed formats).
- If you are unsure about the current state of the ecosystem, say so explicitly rather than inventing.

## 2. Modularity only at real seams

An abstraction must pay rent. Introduce an interface/trait only when at least one holds:

- there are (or will certainly be) two or more real implementations;
- it isolates an external system: storage, transport, third-party API, OS;
- it is required to inject a test double.

Otherwise use concrete types. No speculative interfaces, no `Manager`/`Provider`/`Helper` indirection layers, no "just in case" plugin points. Deleting an unearned abstraction is a valid and welcome refactoring.

## 3. Determinism by construction

All entropy lives at the composition root — the warmup/setup layer: current time, randomness, UUID generation, environment, network clients, concurrency configuration. Everything below is deterministic: same inputs → same outputs.

- Inject clock / rng / id-generator. Never call `time.Now()`, `rand`, `uuid.New()` inside domain logic.
- Unit tests must run with no network, no disk, no sleeps, no real time.
- If a component cannot be unit-tested without heavy mocking, the design is wrong. Fix the design, not the test.

## 4. Layers and cognitive load

- Clear responsibility zones: transport → application/service → domain → infrastructure. Dependencies point inward; domain knows nothing about transport or storage.
- Hide implementations behind package boundaries; export the minimum surface.
- Each file/package should be understandable in isolation — the reader must not need the whole call graph in their head.
- Explicit data flow over shared mutable state.

## 5. Built for observation and repair

Assume every component has bugs and every system will eventually need forensics. Observability and manual intervention do not have to exist on day one — but the architecture must make them cheap to add later.

- Observability-ready: significant state transitions pass through identifiable seams (handlers, layer boundaries), not diffused through the code — so metrics, tracing, and structured logs can be attached later without redesign. Emit structured events at boundaries rather than printf from the guts.
- Human-operable: an operator must be able to inspect state and intervene in one moving part — re-run a step, reprocess a single item, correct a single record — without reverse-engineering implicit state. If manual repair requires archaeology, the design has failed.
- Prefer immutable, log-structured state: the source of truth is append-only facts; corrections and derived data are stored as separate, later entities — never in-place overwrites. Derived data is disposable by definition: it must be recomputable from the source of truth.
- Keep provenance on derivatives (which inputs, which code version produced this) so versions can be diffed and rebuilt. Any reprocessing must be idempotent.

## 6. Built to run unattended

Default target: the system runs for months without supervision before anyone has to touch it. Databases and stateful stores are their own discipline — this is about the code itself.

- No leaks: memory, goroutines/tasks, file descriptors, connections. Everything acquired is released on a defined path (defer/RAII, context cancellation); shutdown is graceful and complete.
- No unbounded growth: every queue, cache, map, and retry backlog has a bound and an eviction/expiry policy. Prefer backpressure over buffering.
- Every external interaction has a timeout and bounded retries with jitter. Transient failure is normal operation, not an exception path.
- Steady state is flat: under constant load, memory and descriptor counts must not trend upward. If the operational fix is "restart it periodically", the design has failed.

## 7. General doctrine

- Explicit > implicit. Boring > clever. Deleted code > commented-out code. Flat > nested.
- Errors are values with context: wrap, don't swallow; no panics in library code; fail fast at startup, degrade gracefully at runtime.
- YAGNI applies to features, not to invariants: correctness properties (idempotency, ordering, atomicity, cancellation) are designed up front.
- Concurrency is a design decision, not seasoning: decide state ownership, cancellation, and backpressure before goroutines/tasks appear.
- Optimize with a profile in hand — but don't design in accidental O(n²) or per-request allocations on hot paths.
- Names come from the domain. If a name needs a comment, the name is wrong.
- Small, reviewable changes. One concern per change.

## Pre-flight (mandatory before non-trivial code)

Output 3–6 lines before coding:

1. Problem class.
2. Prior art and industrial options considered (or "genuinely novel, because …").
3. Chosen approach and why.
4. Seams: what is replaceable, what is deliberately concrete.
5. Entropy points hoisted to warmup.
6. Operability: how a human observes this and repairs it by hand when it misbehaves.

Then implement. If the pre-flight reveals the task is bigger than the request implies, say so before writing code.
