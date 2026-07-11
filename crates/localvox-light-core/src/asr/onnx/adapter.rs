//! Trait, описывающий конкретную ASR-модель: mel-параметры, layout входа,
//! имена тензоров и постпроцессинг.

use super::mel::MelConfig;
use super::vocab::Vocab;

/// Как модель ожидает аудио-фичи на входе.
#[derive(Clone, Copy, Debug)]
pub enum InputLayout {
    /// `[batch=1, features, time]` — обычный NeMo Conformer / GigaAM.
    BatchFeatTime,
    /// `[batch=1, time, features]`.
    BatchTimeFeat,
}

/// Конкретный адаптер ONNX-модели. Знает свои mel-параметры, имена тензоров,
/// словарь и как из id-токенов получить текст.
pub trait OnnxAdapter: Send + Sync {
    /// Имя адаптера, например `"gigaam-v3-e2e-ctc"`.
    fn name(&self) -> &str;
    /// Параметры mel-спектрограммы (одинаковы для всего семейства модели).
    fn mel_config(&self) -> &MelConfig;
    /// Layout входного тензора с фичами.
    fn input_layout(&self) -> InputLayout;
    /// Имя входа в ONNX-графе (например, `"audio_signal"`).
    fn input_name(&self) -> &str;
    /// Имя входа с длиной (количеством фрейм-фичей). `None` — если модель не требует.
    fn length_input_name(&self) -> Option<&str>;
    /// Имя выхода с логитами / лог-вероятностями.
    fn output_name(&self) -> &str;
    /// Размер словаря (включая blank).
    fn vocab_size(&self) -> usize;
    /// Словарь с blank-id.
    fn vocab(&self) -> &Vocab;
    /// Преобразовать декодированные id-токены в строку (char-vocab vs SentencePiece — зависит от модели).
    fn detokenize(&self, ids: &[usize]) -> String;
}
