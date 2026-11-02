# Проверить качество правки текста

Инженерная проверка обычного cleanup на [72 собственных фразах](../scripts/fixtures/cleanup-quality-ru.json):
24 однозначных правки и 48 случаев сохранения содержания. Это не оценка качества
всех встреч и не сверка ASR с аудио. Модель и настройки приложения не переключаются.

Нужны собранные [profiler/worker и локальная Ollama](text-postprocessing-profile.md).
Каждый output — новый каталог на D:; STT не запускается.

```powershell
python scripts/check-cleanup-quality.py --prepare target/quality-input-new
.\target\release\localvox-profile.exe --text-only --manifest target/quality-input-new/manifest.json --worker target/release/localvox-process.exe --model-dir models/gigaam-v3-e2e-ctc --output target/quality-gemma-new --repeats 3 --timeout-sec 180 --llm-base-url http://127.0.0.1:11434/v1 --llm-model gemma4:e4b
python scripts/check-cleanup-quality.py --output target/quality-gemma-new --write target/quality-gemma-new/quality.json
```

Статусы каждой фразы:

- `expected`: результат совпал с заранее заданным допустимым вариантом.
- `missed`: исходник сохранён, хотя ожидалась однозначная правка.
- `violation`: пропал обязательный фрагмент либо изменился вопрос/восклицание.
- `review`: другая формулировка; требуется разбор, это ещё не доказанная ошибка.

Exit 2 означает, что остались нарушения, спорные варианты или пропущенные правки.
Один общий процент не смешивает эти причины. Итог проверяет сохранённый результат
после guards, а не безопасность сырых предложений модели. Receipt/source/version/
границы edits проверяются повторно; неполные и sparse профили не принимаются.
Корпус и допустимые варианты фиксируются до native прогонов, не подгоняются под ответы.

Для просмотра уже обработанного публичного материала:

```powershell
python scripts/check-cleanup-quality.py --scan target/public-cleanup-profile --write target/public-review-new.json
```

Этот файл содержит исходные/исправленные строки и соседний контекст. Не добавляйте
пользовательские записи в git. Флаги qualifier/number/literal/question_emphasis/lexical
показывают места для просмотра, а не подтверждённую ошибку. Различайте agent-review,
приёмку владельцем и сверку с аудио; отсутствие флагов не доказывает сохранность смысла.
`adjacent_overlap_flags` отдельно отмечает переносы ≥8 последовательных слов из
соседней исходной строки, которых не было в своей строке; они также требуют разбора.
