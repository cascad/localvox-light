# Измерить правку готового текста

Инженерный режим работает с копией готового JSONL и не запускает STT. Нужны собранные
`localvox-profile`, `localvox-process` и локальная Ollama. `.env`/архив приложения не
читаются. `--output` должен указывать на новый каталог; существующий не затирается.

Для сравнения коротких ответов есть [guarded sparse experiment](sparse-edit-experiment.md).
Для проверки обычной правки — [контрольный набор качества](cleanup-quality.md).
Для разделения цены проверки сводки — [профиль grounding без LLM](summary-grounding-profile.md).

Manifest (пути относительно его файла):

```json
{"version":1,"cases":[{"name":"sample","transcript":"source.jsonl","lang":"ru",
"modes":["refine","cleanup","refine-cleanup","summary"]}]}
```

```powershell
cargo build --release -j 2 -p localvox-light-bench --bin localvox-profile -p localvox-light-asr --bin localvox-process
.\target\release\localvox-profile.exe --text-only --manifest D:\bench\text.json --worker .\target\release\localvox-process.exe --model-dir .\models\gigaam-v3-e2e-ctc --ner-model-dir .\models\ner-gliner --output D:\bench\new-text-run --repeats 1 --timeout-sec 1200 --llm-base-url http://127.0.0.1:11434/v1 --llm-model qwen3.5:9b
```

`--model-dir` сохраняет общий CLI profiler; в text-only ASR не выполняется.
NER используется только для summary. Каждый mode получает отдельную исходную копию;
`refine-cleanup` запускает оба прохода последовательно. Все запросы идут в явно
выбранную локальную Ollama напрямую. Облачного fallback нет.

`report.json`: wall, операции,
проверка receipts и количество принятых/отвергнутых поправок. Отчёт исключает текст,
имена и диагностические сообщения; сами результаты остаются в `sessions/` output.
Вложенные операции не складывают с wall. Число edits не доказывает качество.
Для refine сравниваются сохранённые строки; для cleanup также доступны omitted/rejected.
По умолчанию `trace_enabled=false`, `llm_trace=null`: число запросов, native
load/prompt/generation/tokens и причина завершения не измерены. Wall включает
проверки/запись, это не чистое время генерации. Отсутствующее измерение не заменяется нулём.
Явный `--trace-ollama` включает прежний временный численный перехватчик для
сопоставления с историческими профилями; sparse experiments тоже используют его
по своим явным флагам. Для проверки качества обычного cleanup он не нужен.

При отказе/timeout пачка останавливается с частичным отчётом; незавершённый вызов
не считается полной обработкой. Ctrl+C останавливает принадлежащий инструменту
worker, внешний Ollama может ещё завершать уже принятый запрос. Инструмент не
выгружает чужие модели и не доказывает эксклюзивное владение GPU.
