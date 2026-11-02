# Эксперимент с короткими ответами корректора

Это инженерный прототип; обычные UI/TUI и их модели/настройки не переключаются.
Кандидат умеет узкое оформление и вокальные паузы, а не свободное исправление слов
ASR. Исходные слова сохраняются. Результат вне контрольного набора требует просмотра.

Сборка — как в [text profiler](text-postprocessing-profile.md). Manifest принимает
только `cleanup`. К обычной команде добавить **один** флаг:

- `--experimental-sparse-edits`: JSON `edits: [{line,before,after}]`, маленькие точные
  замены; первый вариант оказался недостаточно полезным в коротких native тестах.
- `--experimental-sparse-lines`: JSON `changes: [{line,text}]`, только изменённые
  строки; точную исходную строку подставляет код.

Используется [JSON schema Ollama](https://docs.ollama.com/capabilities/structured-outputs).
Она ограничивает форму, а смысл проверяется отдельно. Неизвестные/повторные IDs,
невалидный JSON, неоднозначные anchors, пересечения, управляющие символы и изменение
содержательных слов отклоняются. Ошибочная строка возвращается к исходнику.
Отрицания/модальность/вопросы/цитаты сохраняются целиком. В lines-варианте строки
с цифрами и операторами также не изменяются. Новые вопросы/цитаты/голосовые паузы
не добавляются. Это консервативное ограничение, не доказательство всех оттенков смысла.

Пакеты не больше 12 строк / 32000 UTF-8 bytes; многострочный source text не
поддерживается в эксперименте. Sampling/модель сохраняются, num_ctx пересчитывается
под экспериментальный prompt, если требуется больше прежнего окна. Переписывает
запросы/ответы только явно выбранный experimental proxy. Обычный proxy остаётся exact.

Протокол запрашивает короткий JSON, worker получает локально восстановленный полный набор
своих строк и выполняет прежние guards. Native timings/token counters измеряют JSON,
не локально развёрнутый ответ. Worker `omitted=0` не доказывает охват модели: смотрите
`sparse` proposed/accepted/rejected/protocol_errors/eligible_lines в report.json.
Ни исходник, ни before/after не попадают в численный report. Session outputs остаются
в новом каталоге на D:, как и в обычном profiler.

Короткая проверка на [синтетическом corpus](../scripts/fixtures/sparse-edit-quality.json):

```powershell
python scripts/check-sparse-quality.py --prepare target/sparse-corpus-new
.\target\release\localvox-profile.exe --text-only --experimental-sparse-lines --manifest target/sparse-corpus-new/manifest.json --worker target/release/localvox-process.exe --model-dir models/gigaam-v3-e2e-ctc --output target/sparse-result-new --repeats 1 --timeout-sec 120 --llm-base-url http://127.0.0.1:11434/v1 --llm-model gemma4:e4b
python scripts/check-sparse-quality.py --output target/sparse-result-new
```

Оба каталога должны быть новыми. Проверяющий инструмент читает результаты,
сохраняет quality.json и возвращает
exit 2 при провале. Gate: защищённое содержимое сохранено и исправлены ≥6 из 8 явных
вокальных пауз. Один явно эквивалентный порядок слов разрешён отдельно; буквальные
различия не объявляются автоматически смысловой ошибкой. Успех на 24 примерах не
означает общего принятия качества модели. Пустой быстрый ответ не проходит usefulness.
