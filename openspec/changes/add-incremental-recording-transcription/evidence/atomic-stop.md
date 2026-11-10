# Windows atomic publication / Stop, 2026-11-10

В ходе R1 полный core прогон обнаружил один отказ сохранения `stopped_reason`.
Отдельный control test прошёл; затем настоящий test binary запускался 64 раза
с четырьмя параллельными процессами: **63 passed / 1 failed**. Capture journal
неуспешного случая: `Сохранение конца записи`, `meta.json: Access is denied (5)`.
Аудио было закрыто; нормальный повтор полного теста сам по себе не считался фиксом.

[atomic_write](../../../../crates/localvox-light-core/src/artifacts.rs) сохраняет
прежний порядок write→sync→atomic persist. Только на Windows errors 5/32 допускают
повтор того же persist с тем же synced tempfile: 1→2→4→8→10 ms backoff, общий retry
budget 100 ms. Другие ошибки возвращаются сразу, постоянная блокировка — после
исчерпания бюджета. Copy/delete fallback нет; старый destination до успешного
atomic replace остаётся целым. Это budget повторов, не обещание прервать зависший
системный I/O. Unix путь не изменён.

Два native Windows tests с настоящим handle без share-delete: первый persist
отказывает, handle освобождается, второй публикует полный файл; при постоянном
handle операция ограниченно отказывает, исходные bytes неизменны. Оба passed.
Затем та же серия control test binary: **64 passed / 0 failed**. Numeric report
в ignored `target/control-stop-stress-20261110.json`; полный core: 441 passed,
7 ignored; API: 79 passed, 6 ignored; application/worker/TUI check passed.

Control test теперь включает capture journal в assertion failure для воспроизводимой
диагностики. Порог/ожидания/assertion не ослаблены. Это временные WAV/seals и API
domain без устройств/моделей; рабочий daemon и архив не затрагивались. REC-003 и
INV-REC-002 проверены в указанной синтетической границе; hardware/RV-014 не закрыты.
