# Back navigation · 2026-11-14

Владелец просмотрел demo: «а так круто выглядит»; оформление одобрено.
Обнаружен UX gap: в первоначальном мастере нельзя было исправить предыдущий ответ
без полной отмены. Этот отчёт дополняет [первый прогон](preview.md), не заменяет его.
SETUP-009 остаётся delta; real install/bootstrap/публикация не подключены.

## Implementation

[wizard.rs](../../../../crates/localvox-setup-ui/src/wizard.rs) хранит отдельный draft
подтверждённых ответов и историю посещённых Step. Back возвращает по истории;
дальнейшие вопросы вычисляются по текущим ответам. Custom draft переживает переход
в recommended и обратно. Поля разных LLM sources и введённые пути не сбрасываются.

На промежуточном шаге Esc/Ctrl+C открывает «Назад / Продолжить текущий шаг /
Отменить установку». На первом экране — выход, повторный Esc/Ctrl+C в меню — выход.
В плане есть явный «Назад». Подсказка видна внизу, в том числе при validation error.
Сохраняются подтверждённые ответы; прерванный незавершённый ввод не принимается.
Используется публичный cliclack API: единый Interrupted не разбирается через
перехват stdin/приватные поля/изменения библиотеки.

US-064 и [инструкция](../../../../docs/installer-preview.md) обновлены.

## Executed checks

Windows x86_64 / Rust 1.99.1, изолированные build/cache/TEMP на D:.

```powershell
cargo test --locked -p localvox-setup-ui --target x86_64-pc-windows-msvc --jobs 2 -- --test-threads=1
cargo fmt -p localvox-setup-ui --check
node --check scripts/installer-preview/capture.mjs
node scripts/installer-preview/capture.mjs
```

С теми же переменными окружения, что в первом отчёте: **10 tests passed**, 5.63 s,
13 реальных ConPTY sessions плюс pipe/help. Новые четыре сценария проверили:

- Back из плана, custom → recommended → custom с сохранением галочек;
- Back из выбора модели и отключение summary без устаревших вопросов;
- Back к введённым папкам, изменение одной и сохранение другой/model tag;
- Stay на текущем шаге и явную отмену в ASCII терминале шириной 80.

Существующие default/custom/Ollama/server/GGUF/cancel проверки прошли.
Каждая PTY-сессия проверила побайтную неизменность private home/config/model fixture
и восстановленный курсор. Fmt и Node syntax passed. Clippy остаётся not-run:
компонент отсутствует в изолированном toolchain.

Сняты 6 PNG и обновлённый GIF с Esc → Back; новый экран навигации и план просмотрены
агентом. В `target/review/installer-preview/` подготовлен
`localvox-setup-preview-v2.exe`; его `--help` passed. Старый `.exe` открыт владельцем
и удерживает файл: не останавливался, новая сборка положена рядом.

Strict main specs — 15 passed / 0 failed; strict change — valid.
`python scripts/check-openspec.py` — PASS: 15 capabilities / 88 requirements /
188 scenarios / 88 coverage rows / 2059 local links. `git diff --check` passed.
SHA-256 проверенного build и выданной v2 копии совпали; ffprobe подтвердил GIF
1240×930, 6 кадров / 12 секунд.

Новая навигация владельцем, native Linux/macOS — **not-run**. Stage 1.2 остаётся
открытым; push/GitHub build/production installation не выполнялись.
