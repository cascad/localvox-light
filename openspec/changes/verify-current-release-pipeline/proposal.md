# Proposal

Статус: свежий комплект собран и программно проверен 2026-11-09;
[результаты и ограничения](evidence/results.md). Оба native маршрута завершены
успешно; аппаратная/смысловая приёмка не выполнена, daemon не запускался.

## Why

После fixes сохранности правки и [ускорения NER](../skip-irrelevant-role-windows/evidence/results.md)
рабочий release worker оставался прежним. Требуется свежий согласованный комплект
и проверка полного STT/optional summary маршрута, а не только отдельных компонентов.

## What Changes

- Собрать UI, desktop, daemon, worker, TUI и существующие профилировщики из текущих
  исходников; сохранить build hashes и результаты программных regression tests.
- Проверить две fresh сессии прежнего публичного двухчасового PCM: STT-only и
  STT+summary с текущими Ollama/qwen3.5:9b, без включения refine/cleanup.
- Использовать существующий headless production coordinator/worker с новым
  помеченным тестовым архивом на D:, bounded timeout, численными clocks/receipts.
- Проверить реальные переходы, сохранность результатов, отсутствие лишних этапов;
  отказ/timeout/cancellation проверить штатным model-free runtime fixture.
- Обновить evidence/coverage и готовность сборки отдельно от качества содержания,
  аппаратного звука, Mac и обновления/запуска рабочего демона.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. Build/verification/evidence для PIPE-004/007/008/009/010/011 и LLM-004/005
без новых требований и продуктового поведения; `skip_specs: true`.

## Impact

Артефакты сборки, изолированные тестовые данные и численные отчёты на D:, актуальные
OpenSpec evidence. Модели, пользовательские settings/.env и архив не менять.
Не запускать микрофон/захват и не заменять проверку качества фактом ready. При
обнаруженном дефекте исправление вести по контракту; не вводить хрупкие обходы.
