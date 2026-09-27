## ADDED Requirements

### Requirement: INV-RUN-001 Component shutdown diagnostics

Остановка демона SHALL устанавливать сигнал отмены до ожидания компонентов.
Ожидание SHALL сообщать имя компонента и прошедшее время; превышение лимита
или panic SHALL не сообщаться как успешный штатный выход. Ошибка остановки одного
компонента SHALL не пропускать попытку сохранить и завершить остальные.

#### Scenario: Worker coordinator exceeds shutdown limit

- **WHEN** autocook не завершился за отведённое время
- **THEN** ошибка явно называет autocook, а завершение engine всё равно запрашивается и проверяется.

#### Scenario: Engine thread panics

- **WHEN** ожидаемый поток engine завершился panic
- **THEN** это диагностируется как ошибка, не обычное завершение.
