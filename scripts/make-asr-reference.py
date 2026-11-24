#!/usr/bin/env python3
"""Development-only ONNX/Transformers references; never used by the installed app.

Requires numpy, onnxruntime, torch and transformers. Weights/audio must already
exist locally; this script never downloads models or opens capture devices.
"""
import argparse
from itertools import groupby
import json
from pathlib import Path
import wave


def tone_reference(directory, audio):
    import numpy as np
    import onnxruntime as ort
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    model = ort.InferenceSession(str(directory / 'model.onnx'), options, providers=['CPUExecutionProvider'])
    cases = []
    for file in audio:
        with wave.open(str(file), 'rb') as wav:
            if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (8000, 1, 2):
                raise ValueError('Reference audio must be PCM16 mono 8 kHz: ' + str(file))
            samples = np.frombuffer(wav.readframes(wav.getnframes()), dtype='<i2').astype(np.int32)
        frame = 2400
        padded = np.pad(samples, (frame, frame))
        padded = np.pad(padded, (0, (-len(padded)) % frame))
        state = np.zeros((1, 219729), dtype=np.float16)
        tokens = []
        for offset in range(0, len(padded), frame):
            logits, state = model.run(None, {'signal': padded[offset:offset+frame].reshape(1, frame, 1), 'state': state})
            tokens.extend(logits.argmax(axis=-1).reshape(-1).tolist())
        labels = 'абвгдеёжзийклмнопрстуфхцчшщъыьэюя '
        text = ''.join(labels[token] for token, _ in groupby(tokens) if token < len(labels)).strip()
        cases.append({'wav': str(file.resolve()), 'text': text})
        print(file.name, text, flush=True)
    return {'model_dir': str(directory.resolve()), 'cases': cases}


def sbert_reference(directory):
    import numpy as np
    import torch
    from transformers import AutoModelForTokenClassification, AutoTokenizer
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    tokenizer = AutoTokenizer.from_pretrained(directory, local_files_only=True, strip_accents=False)
    model = AutoModelForTokenClassification.from_pretrained(directory, local_files_only=True).float().eval()

    def classify(words):
        encoded = tokenizer(words, is_split_into_words=True)
        if len(encoded.input_ids) > 512:
            if len(words) < 2: raise ValueError('one word exceeds the model limit')
            middle = (len(words)+1)//2
            left, _ = classify(words[:middle])
            right, _ = classify(words[middle:])
            return left + right, None
        with torch.inference_mode():
            logits = model(input_ids=torch.tensor([encoded.input_ids]),
                           attention_mask=torch.tensor([encoded.attention_mask])).logits[0].numpy()
        rendered = []
        word_ids = encoded.word_ids()
        for word_index, word in enumerate(words):
            label = int(np.argmax(logits[word_ids.index(word_index)]))
            if not 1 <= label <= 12: raise ValueError(f'Unmapped upstream label: {label}')
            casing, mark = divmod(label-1, 4)
            result = word if casing == 0 else word.capitalize() if casing == 1 else word.upper()
            result += ('', '.', ',', '?')[mark]
            rendered.append(result)
        return rendered, {'token_ids': encoded.input_ids, 'word_ids': word_ids, 'logits': logits.tolist()}

    cases = []
    for text in ['привет как дела сегодня мы проверяем новую модель',
                 'ёлка ёжик москва api иван иванов 2026 12345',
                 'встреча состоится 25 ноября в 15 часов приходите вовремя',
                 'сегодня проверяем числа 3.14 и 1000000 спасибо',
                 ' '.join(['проверяем длинную запись чтобы не потерять ни одного слова'] * 80)]:
        rendered, detail = classify(text.strip().lower().split())
        cases.append({'text': text, 'rendered': ' '.join(rendered), **(detail or {})})
        print('Sbert', len(text.split()), 'words', flush=True)
    return {'model_dir': str(directory.resolve()), 'cases': cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tone-dir', type=Path)
    parser.add_argument('--sbert-dir', type=Path)
    parser.add_argument('--audio', type=Path, action='append', default=[])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for name, directory in [('tone', args.tone_dir), ('sbert', args.sbert_dir)]:
        if directory:
            data = tone_reference(directory, args.audio) if name == 'tone' else sbert_reference(directory)
            (args.output / (name + '-reference.json')).write_text(json.dumps(data, ensure_ascii=False), encoding='utf-8')


if __name__ == '__main__': main()
