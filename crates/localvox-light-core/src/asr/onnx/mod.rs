//! ONNX-based ASR engine. CPU-friendly, поддерживает разные архитектуры через адаптеры.
//!
//! Слои:
//! * [`mel`] — log-mel препроцессор (HTK-шкала, NeMo-совместимые параметры).
//! * [`ctc`] — жадное CTC-декодирование.
//! * [`vocab`] — загрузка `vocab.txt` формата onnx-asr и SentencePiece-детокенизация.
//! * [`adapter`] — trait [`OnnxAdapter`] описывает конкретную модель.
//! * [`engine`] — [`OnnxEngine`] оборачивает `ort::Session` + адаптер,
//!   реализует общий [`crate::asr::AsrEngine`].
//! * [`adapters`] — реализации адаптеров (GigaAM v3 и др.).

pub mod adapter;
pub mod adapters;
pub mod ctc;
pub mod engine;
pub mod mel;
pub mod vocab;

pub use adapter::{InputLayout, OnnxAdapter};
pub use engine::{inspect_model_io, OnnxEngine, OnnxIoInfo};
pub use mel::{MelConfig, MelSpectrogram};
pub use vocab::Vocab;
