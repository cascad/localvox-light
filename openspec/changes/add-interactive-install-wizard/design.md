# Design

## Context

См. proposal.md. Текущий setup уже разделяет plan/probe/execute, но печатает raw
doctor и числовой prompt. Владелец уточнил single-command bootstrap и два режима,
просит увидеть имитатор до настоящего применения и GitHub build.

## Goals / Non-Goals

Goals: самостоятельный лёгкий frontend и локальный demo; далее повторное
использование frontend с installer controller и существующими проверками/действиями.
Non-goals первого этапа: исправление production setup, настоящие downloads,
публикация release/tag/push, перенос существующих данных, новый ASR/LLM benchmark.

## Decisions

- Отдельный workspace crate setup-ui с typed view data/selection и preview binary.
  Он не импортирует daemon/core/audio/Vosk, поэтому demo быстро собирается на трёх ОС.
- Использовать готовые CLI prompts (cliclack), не писать свой raw-input/TTY parser.
  Шаги: приветствие → default/custom → custom checklist/папки/модель → итог → demo.
  Обязательный STT выделен отдельно, optional управляются Space. Готовое не удаляется.
- Навигация: явные Step/Move, история посещённых шагов и отдельный draft ответов.
  Back возвращает по истории; следующий шаг вычисляется заново по актуальному draft.
  В итоговом плане есть «Назад», на промежуточных Esc открывает Back/Stay/Cancel.
  Cliclack возвращает единый Interrupted для Esc/Ctrl+C: используем одно явное меню,
  не перехватываем stdin и не патчим parser/приватные internals библиотеки.
  Сохраняются подтверждённые ответы; незавершённый ввод при прерывании не принимается.
- Данные о найденном, путях, размерах и рекомендованной модели приходят от controller;
  preview использует явно фиктивные fresh/existing сценарии, без probes системы.
  Default: STT, голоса, сводки, ссылки и TUI player; NER — custom optional.
- Снятие галочек не разрешает удалять существующие программы/веса. Runtime toggle
  и download policy соединяются в production controller после просмотра demo.
- Bootstrap должен обходиться без Rust/npm/jq со стороны конечного пользователя:
  PS/sh получают native installer/полный release, а не заставляют искать Actions.
  Версия/hash/platform согласованы; публикация полного release — отдельный gated этап.
- Существующий планировщик не дублировать в preview. Следующий этап адаптирует typed
  selections к единственному mutation boundary; не автоматизировать ввод старых цифр.
- Локальный стенд использует tiny payloads/свой HTTP, temporary home/config и fake
  Ollama; native Linux/Mac владелец запустит отдельно. Не устанавливать реальные веса.

## Risks / Trade-offs

Нет полного опубликованного release → одна строка не объявляется доступной до доставки.
TTY и clipboard/input → ConPTY тест и реальные снимки; pipe не изображает интерактивность.
Default модель и размеры → явные данные каталога/найденное, никаких выдуманных totals.
Preview review → после первого этапа остановиться на просмотре владельца.
