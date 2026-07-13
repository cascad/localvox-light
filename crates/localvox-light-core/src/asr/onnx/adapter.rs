//! The trait that describes a concrete ASR model: mel parameters, input layout, tensor
//! names and post-processing.

use super::mel::MelConfig;
use super::vocab::Vocab;

/// How the model expects the audio features on its input.
#[derive(Clone, Copy, Debug)]
pub enum InputLayout {
    /// `[batch=1, features, time]` — the usual NeMo Conformer / GigaAM.
    BatchFeatTime,
    /// `[batch=1, time, features]`.
    BatchTimeFeat,
}

/// A concrete adapter of an ONNX model. It knows its mel parameters, the tensor names, the
/// vocabulary and how to turn token ids into text.
pub trait OnnxAdapter: Send + Sync {
    /// The name of the adapter, for example `"gigaam-v3-e2e-ctc"`.
    fn name(&self) -> &str;
    /// The parameters of the mel spectrogram (the same for the whole model family).
    fn mel_config(&self) -> &MelConfig;
    /// The layout of the input tensor with the features.
    fn input_layout(&self) -> InputLayout;
    /// The name of the input in the ONNX graph (for example, `"audio_signal"`).
    fn input_name(&self) -> &str;
    /// The name of the length input (the number of frame features). `None` — if the model
    /// does not require it.
    fn length_input_name(&self) -> Option<&str>;
    /// The name of the output with the logits / log-probabilities.
    fn output_name(&self) -> &str;
    /// The size of the vocabulary (including the blank).
    fn vocab_size(&self) -> usize;
    /// The vocabulary with the blank id.
    fn vocab(&self) -> &Vocab;
    /// Turn the decoded token ids into a string (char-vocab vs SentencePiece — depends on
    /// the model).
    fn detokenize(&self, ids: &[usize]) -> String;
}
