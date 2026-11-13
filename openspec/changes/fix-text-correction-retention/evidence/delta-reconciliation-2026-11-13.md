# Delta coherence, 2026-11-13

[Interface run 43447219612](https://github.com/cascad/localvox-light/actions/runs/43447219612)
на `44df41e` прошёл UI 81 e2e, native audio regression, все Rust/client/worker
шаги, совместный UI/TUI и media/FLAC проверки. Specification checks failed:
старый MODIFIED delta не содержал три сценария, уже присутствующие в main specs.

Тот же strict validator локально воспроизвёл failure. Из действующего main
контракта дословно возвращены два LLM-003 сценария и один PIPE-006 сценарий.
Основной контракт, реализация guard, исторические результаты и tasks не менялись.
При будущем sync/archive старый delta больше не удалит принятые сценарии.

После исправления: strict main specs и **все 39 change validations**, перечисленные
в interface workflow, passed. Лог `target/tmp/interface-all-spec-checks.log`.
Check-openspec и diff также проверяются перед commit. Новый hosted run not-run
до push владельца; исходный run целиком не объявляется green.
