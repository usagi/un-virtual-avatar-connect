"""Export the pinned english2kana Keras model to ONNX and prove parity.

This script is only run inside the conversion venv created by cargo xtask.
UNVAC build and runtime never import Python, TensorFlow, or Keras.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

os.environ.setdefault("KERAS_BACKEND", "tensorflow")
os.environ.setdefault("TF_CPP_MIN_LOG_LEVEL", "2")

import keras
import numpy as np
import onnxruntime as ort
import tensorflow as tf
import tf2onnx

MAX_ENGLISH = 29
MAX_KANA = 36
START_TOKEN = "<s>"
END_TOKEN = "<e>"
GOLDEN_WORDS = (
    "hello",
    "world",
    "twitch",
    "openai",
    "github",
    "arknights",
    "doctor",
    "streaming",
    "english",
    "mvp",
)


@keras.saving.register_keras_serializable(name="DotAttention")
class DotAttention(keras.layers.Layer):
    """Upstream custom layer, kept byte-for-byte equivalent in behavior."""

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.supports_masking = True

    def call(self, inputs, mask=None):
        decoder_outputs, encoder_outputs = inputs
        encoder_outputs_transposed = tf.transpose(encoder_outputs, perm=[0, 2, 1])
        score = tf.matmul(decoder_outputs, encoder_outputs_transposed)
        if mask is not None:
            _, encoder_mask = mask
            if encoder_mask is not None:
                encoder_mask = tf.cast(encoder_mask, dtype=score.dtype)
                encoder_mask = tf.expand_dims(encoder_mask, axis=1)
                score += (1.0 - encoder_mask) * -1e9
        attention_weights = tf.nn.softmax(score, axis=-1)
        context_vector = tf.matmul(attention_weights, encoder_outputs)
        return context_vector, attention_weights

    def compute_mask(self, inputs, mask=None):
        del inputs
        if mask is None:
            return None
        decoder_mask, _ = mask
        return decoder_mask


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--english-tokenizer", required=True, type=Path)
    parser.add_argument("--kana-tokenizer", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    return parser.parse_args()


def tokenizer_maps(path: Path) -> tuple[dict[str, int], dict[int, str]]:
    outer = json.loads(path.read_text(encoding="utf-8"))
    config = outer["config"]
    word_index = {key: int(value) for key, value in json.loads(config["word_index"]).items()}
    index_word = {int(key): value for key, value in json.loads(config["index_word"]).items()}
    return word_index, index_word


def encode_word(word: str, word_index: dict[str, int]) -> np.ndarray:
    normalized = word.lower()
    ids = [word_index[char] for char in normalized if char in word_index]
    encoded = np.zeros((1, MAX_ENGLISH), dtype=np.float32)
    encoded[0, : min(len(ids), MAX_ENGLISH)] = ids[:MAX_ENGLISH]
    return encoded


def decode_ids(ids: list[int], index_word: dict[int, str]) -> str:
    text = "".join(index_word.get(value, "") for value in ids if value != 0)
    # Match upstream `_decode_sequence`: str.lstrip/rstrip receive a character
    # set, not a complete marker. This removes the separately tokenized `<s`.
    text = text.lstrip(START_TOKEN).rstrip(END_TOKEN)
    return text.replace("<", "").replace(">", "")


def keras_translate(model, encoder, start_id, end_id, index_word):
    output_ids = [start_id]
    probabilities = []
    tensors = []
    for _ in range(MAX_KANA - 1):
        decoder = np.zeros((1, MAX_KANA), dtype=np.float32)
        decoder[0, : len(output_ids)] = output_ids
        prediction = np.asarray(model([encoder, decoder], training=False))
        tensors.append(prediction)
        step = prediction[0, len(output_ids) - 1]
        predicted_id = int(np.argmax(step))
        probabilities.append(float(step[predicted_id]))
        if predicted_id in (0, end_id):
            break
        output_ids.append(predicted_id)
    return decode_ids(output_ids, index_word), output_ids, probabilities, tensors


def onnx_translate(session, input_names, encoder, start_id, end_id, index_word):
    output_ids = [start_id]
    probabilities = []
    tensors = []
    for _ in range(MAX_KANA - 1):
        decoder = np.zeros((1, MAX_KANA), dtype=np.float32)
        decoder[0, : len(output_ids)] = output_ids
        prediction = session.run(None, {input_names[0]: encoder, input_names[1]: decoder})[0]
        tensors.append(prediction)
        step = prediction[0, len(output_ids) - 1]
        predicted_id = int(np.argmax(step))
        probabilities.append(float(step[predicted_id]))
        if predicted_id in (0, end_id):
            break
        output_ids.append(predicted_id)
    return decode_ids(output_ids, index_word), output_ids, probabilities, tensors


def main() -> None:
    args = parse_args()
    english_word_index, _ = tokenizer_maps(args.english_tokenizer)
    kana_word_index, kana_index_word = tokenizer_maps(args.kana_tokenizer)
    # The upstream tokenizer is character-level, so neither marker exists as a
    # complete token. Preserve English2KanaInferer.translate's exact defaults.
    start_id = kana_word_index.get(START_TOKEN, 1)
    end_id = kana_word_index.get(END_TOKEN, 0)

    reference_model = keras.models.load_model(
        args.model,
        compile=False,
        custom_objects={"DotAttention": DotAttention},
        safe_mode=True,
    )
    # TensorFlow lowers the default Keras LSTM to ONNX Loop. tract intentionally
    # does not implement that control-flow op. Both sequence lengths are fixed,
    # so clone the network with unrolled LSTMs and preserve the exact weights.
    def clone_for_onnx(layer):
        config = layer.get_config()
        if isinstance(layer, keras.layers.LSTM):
            config["unroll"] = True
        return layer.__class__.from_config(config)

    model = keras.models.clone_model(reference_model, clone_function=clone_for_onnx)
    model.set_weights(reference_model.get_weights())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    input_signature = (
        tf.TensorSpec((None, MAX_ENGLISH), tf.float32, name="encoder_input"),
        tf.TensorSpec((None, MAX_KANA), tf.float32, name="decoder_input"),
    )

    @tf.function(input_signature=input_signature)
    def serving_fn(encoder_input, decoder_input):
        return model([encoder_input, decoder_input], training=False)

    tf2onnx.convert.from_function(
        serving_fn,
        input_signature=input_signature,
        opset=17,
        output_path=str(args.output),
    )

    session = ort.InferenceSession(str(args.output), providers=["CPUExecutionProvider"])
    input_names = [item.name for item in session.get_inputs()]
    if len(input_names) != 2:
        raise RuntimeError(f"expected two ONNX inputs, got {input_names}")

    cases = []
    overall_max_error = 0.0
    all_equal = True
    for word in GOLDEN_WORDS:
        encoder = encode_word(word, english_word_index)
        keras_result = keras_translate(reference_model, encoder, start_id, end_id, kana_index_word)
        onnx_result = onnx_translate(session, input_names, encoder, start_id, end_id, kana_index_word)
        compared_steps = min(len(keras_result[3]), len(onnx_result[3]))
        max_error = max(
            (float(np.max(np.abs(keras_result[3][i] - onnx_result[3][i]))) for i in range(compared_steps)),
            default=0.0,
        )
        equal = keras_result[0] == onnx_result[0] and keras_result[1] == onnx_result[1]
        all_equal = all_equal and equal
        overall_max_error = max(overall_max_error, max_error)
        cases.append(
            {
                "input": word,
                "keras": keras_result[0],
                "onnx": onnx_result[0],
                "token_ids_equal": keras_result[1] == onnx_result[1],
                "confidence": float(
                    np.exp(np.mean(np.log(np.maximum(onnx_result[2], np.finfo(np.float32).tiny))))
                ),
                "max_abs_error": max_error,
            }
        )

    report = {
        "all_equal": all_equal,
        "max_abs_error": overall_max_error,
        "inputs": [{"name": item.name, "shape": item.shape, "type": item.type} for item in session.get_inputs()],
        "outputs": [{"name": item.name, "shape": item.shape, "type": item.type} for item in session.get_outputs()],
        "cases": cases,
    }
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if not all_equal:
        raise RuntimeError("Keras and ONNX greedy decoding differ; see verification report")


if __name__ == "__main__":
    main()
