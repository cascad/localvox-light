# Срез 2 · UI/API feature operations, 2026-11-11

DocumentPane теперь собирает представления из document/*: language, transcript,
summary, readable, provenance, speakers и partial status. Controller владеет
polling/copy/control/recook. Tab/type и toTurns re-export сохранены для callers;
DOM/class names, тексты, условия, copy guards и правила partial не менялись.

Archive сохраняет публичный фасад/DTO. 52 операции сгруппированы в catalog,
documents, commands, recording, participants, queries, notes, queue и media.
Profiles/env builders выделены в config и не зависят от фасада. Lease, artifact
verification, async enqueue и ошибки остаются внутри тех же операций; HTTP routes
и serialization не менялись. Перенос не закрывает BL-006 и прочие старые gaps.

## Test-run

- UI production build и **14 unit passed**.
- **80 browser e2e passed**, updateSnapshots=none, два worker: все прежние visual
  baselines, physical copy/editor guards, growing partial/selection/scroll/playing
  snapshot и finalization проверены без обновления PNG.
- API full package: **81 unit passed / 8 ignored**, journal/RRF/semantic overload
  integrations passed. Существующие fault/mutations/partial/HTTP/settings tests
  сохраняют ограничения записи/отмены/receipt и public error responses.
- TUI из предыдущего среза сохраняет тот же HTTP DTO; новый общий прогон обоих
  клиентов запланирован в 4.2. Устройства/Mac/настоящие LLM не проверялись.

Логи: ignored target/tmp/modular-ui-{build,unit,e2e}.log и modular-api.log.
User-guide не меняется: действий и поведения новых нет. Список вызовов и исходные
тела переносимых методов проверены; warning неиспользуемого import в config убран.

Strict main 15/0, strict change valid (skip_specs), check-openspec PASS
(15/88/188, 1866 links); scoped rustfmt/diff прошли.
