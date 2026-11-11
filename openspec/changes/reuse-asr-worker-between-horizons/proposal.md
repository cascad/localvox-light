# Proposal

## Why

Native reference раннего распознавания показал +48.12% active wall. После разбора
владелец поручил исправить повторные загрузки ASR и лишние проверки кеша. ASR calls
не повторялись; 24 model loads, 16.2 GB чтений весов и повторные обходы checkpoints
являются отдельной ценой реализации. Основание: BL-008 и R4/PIPE-015/016/018.

## What Changes

- Один управляемый ASR-процесс на существующую Transcribe lane, последовательные
  finite prefix/final requests с повторным использованием одного engine.
- Ограниченный keep-alive (default 600 s, 0 выключает), остановка дерева при
  cancel/timeout/crash/shutdown, idle eviction и явная проверка версии протокола.
- Fingerprint весов проверяется на запрос; изменившаяся identity заменяет engine.
  Проверка после загрузки остаётся; доверия только mtime нет.
- Checkpoint не перечитывает один результат несколько раз в одной операции;
  immutable verified batch передаётся в публикацию, recovery/corruption checks остаются.
- Ограниченный durable result journal заменяет temp/rename каждого окна; технический
  range уходит в debug, счётчики журнала обновляются раз в секунду и на границах порции.
- Native alternating reference с прежним PCM/окнами/calls/gate, fault/integration
  regressions, документация настроек и доказательства. Старые результаты сохраняются.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `processing-pipeline`: PIPE-019 — управляемое сохранение ASR engine между запросами.

## Impact

process supervisor, worker protocol, ASR/core cook/cache, coordinator lane, defaults
и profiler. GPU/LLM/модель/рецепт окон не меняются. Default раннего режима сохраняется
off до отдельного подтверждённого принятия gates; аппаратная приёмка не подменяется.
