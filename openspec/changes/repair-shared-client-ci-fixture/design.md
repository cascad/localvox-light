# Design

## Context

См. proposal.md. Внешний тест копирует libtest helper во временный каталог,
поднимающий HTTP, настоящий TUI через ConPTY и Chromium. `ManagedProcess`
глушит stdout/stderr, а внешний тест читает только sentinel. Hosted artifacts
показывают прогресс обоих клиентов без panic hook, поэтому причина пока неизвестна.

## Goals / Non-Goals

**Goals:** воспроизведение и диагностика всего owned process tree, сохранение
проверок API/archive, чтения, заметок, настроек, reconnect и client-only exit.

**Non-Goals:** изменение контрактов продукта, запуск моделей/записи, обновление
визуальных baseline, пропуск проблемного шага или увеличение timeout без причины.

## Decisions

- Использовать существующий bounded supervisor с файловыми stdout/stderr для
  конечного helper, если подтвердится потеря диагностики. Это сохраняет ownership
  и timeout; прямой неуправляемый spawn оставил бы потомков при сбое.
- Определять успех по exit status и sentinel: один sentinel скрывает native abort.
- Исправлять источник воспроизводимого сбоя, сохраняя реальные UI/TUI и HTTP.

## Risks / Trade-offs

- Windows native abort может не вызывать panic hook → сохранять stderr и status.
- Тест зависит от готовых UI/TUI/Chromium → проверять их наличие, без подмены mocks.
- Локальный green не доказывает hosted green → отдельные ссылки и статусы evidence.
