//! ONNX-based ASR engine. CPU-friendly, supports different architectures via adapters.
//!
//! The layers:
//! * [`mel`] — the log-mel preprocessor (HTK scale, NeMo-compatible parameters).
//! * [`ctc`] — greedy CTC decoding.
//! * [`vocab`] — loading a `vocab.txt` in the onnx-asr format and SentencePiece
//!   detokenization.
//! * [`adapter`] — the trait [`OnnxAdapter`] describes a concrete model.
//! * [`engine`] — [`OnnxEngine`] wraps an `ort::Session` + an adapter and implements the
//!   common [`crate::asr::AsrEngine`].
//! * [`adapters`] — the adapter implementations (GigaAM v3 and others).

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
