# OpenSpec — контракт поведения LocalVox

**`specs/` — принятый источник правды**, с 2026-09-27.
[Основание и границы принятия](baseline.md).
Инвентаризация извлечена из кода, перепроверена и преобразована в требования.
При расхождении исправляем код по контракту либо явно принимаем изменение поведения.

Принятие спека не означает, что все баги уже исправлены. У каждого требования есть
сценарии, код, свидетельство и известные ограничения. README и старые документы —
руководства и история, а не конкурирующие требования.

## Навигация

| Что нужно | Где |
|---|---|
| Список всех возможностей | [Каталог](catalog.md) → `specs/<capability>/spec.md` |
| Весь путь источника до результата | [Полный пайплайн](specs/processing-pipeline/spec.md) |
| Что принято, как связаны DEC и требования | [Baseline](baseline.md) |
| Требование → код → проверки → разрыв | [Матрица покрытия](evidence/coverage.md) |
| Известные дефекты и непроверенные интеграции | [Review](review.md) |
| Точные результаты команд/инцидентов | [Журнал сверки](evidence/implementation-review-2026-09-27.md) |
| Изменения в работе или на обсуждении | `changes/`, [указатель](catalog.md#предложения) |
| Сырые идеи | [Backlog/inbox](backlog.md) |
| Процессы и диагностика | [Архитектура](../docs/architecture.md), [памятка](../docs/processing-pipeline.md) |
| История переноса | [Миграция](evidence/migration.md), [официальный workflow](evidence/native-workflow.md), [ADR-0002](../docs/adr/0002-openspec-code-inventory.md) |

## Как читать статус

- `accepted` — контракт обязателен в указанном объёме; не гарантия готовности сборки.
- `code-reviewed` — реализация прочитана; нет утверждения о выполнении сценария.
- `test-linked` — тест найден; `test-run` — конкретный тест выполнен с результатом.
- `runtime-mock` — бинарник проверен на временных данных и подставном назначении.
- `not-run` — соответствующего прогона нет.
- `GAP` — код нарушает принятое правило; ссылка ведёт к RV/change.

`Requirements` и `Scenario` в main specs задают обязательное поведение.
`Implementation status` и `Observed limitations` описывают текущий код и пределы
проверки; они не отменяют требования. Новые хотелки находятся в changes/backlog,
не маскируются под существующие функции.

## Штатный рабочий процесс

1. Найти capability и ID требования; прочитать сценарий и строку evidence/coverage.md.
2. Для бага воспроизвести расхождение с контрактом. Для нового поведения — описать
   намерение в proposal; сырые идеи можно оставлять proposal-only до груминга.
3. Использовать официальный CLI и skills: explore/new/continue/propose/update
   для планирования, apply для реализации, verify для сверки.
4. Обновить код, существенные тесты и evidence затронутых требований. Только после
   фактического выполнения отмечать задачи. Согласованные DEC не спрашивать заново.
5. Выполненное изменение синхронизировать и архивировать через sync/archive;
   невыполненные proposals не выдавать за реализацию. Однократное принятие baseline
   с известными gaps описано отдельно в baseline.md.

Concrete changes живут в `changes/`; backlog — вспомогательный Markdown inbox,
не второй трекер OpenSpec. `status` показывает документы, `instructions apply` —
задачи. `isPlanningComplete` не означает, что функция реализована.
BL-009 — аудит/документация с `skip_specs: true`; поведенческие исправления ведутся
отдельно. Не используем skip для сокрытия недописанных будущих требований.

## Команды проверки

Из корня репозитория:

```powershell
$env:OPENSPEC_TELEMETRY='0'
openspec list --specs
openspec list
openspec status --change review-code-inventory --json
openspec instructions apply --change review-code-inventory --json
openspec validate --specs --strict --no-interactive
python scripts/check-openspec.py
```

Main specs: 15 capability, 86 требований. Матрица покрывает каждый ID.
Проверка формы/ссылок не заменяет проверку логики или benchmark.

Для готового change:
`openspec validate <name> --type change --strict --no-interactive`.
`validate --all` включает незавершённые proposals: три поведенческих черновика
(recover-corrupt-processing-artifacts, unify-manual-and-queued-import,
persist-notes-before-delivery) пока без delta и не проходят полный валидатор.
Они остаются видимыми, не объявляются законченными ради зелёного результата.

## Официальные skills

Установлен CLI **1.13.2**, схема `spec-driven`, девять официальных skills в
[.agents/skills](../.agents/skills); Node >=20.19.0.
Проектные правила находятся в AGENTS.md/config.yaml, сгенерированные skills не правим.
В Codex можно назвать `$openspec-verify-change review-code-inventory` или описать
задачу обычным языком. Change выбирается по задаче; укажите другое имя для переключения.

## Воспроизведение установки

```powershell
npm install --global @fission-ai/openspec@1.13.2
$env:OPENSPEC_TELEMETRY='0'
openspec config set workflows '["explore","new","continue","propose","apply","update","verify","sync","archive"]'
openspec config set profile custom
openspec init --tools codex --profile custom --no-animation
```

Workflow profile — глобальная настройка CLI. После смены версии skills обновляются
через `openspec update`. Ссылки в `~/.agents/skills` ведут на проектные официальные
файлы; при переносе репозитория их нужно обновить.
Исторические evidence сохраняют свои даты и не переименовываются в новые прогоны.
