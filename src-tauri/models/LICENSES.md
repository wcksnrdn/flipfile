# AI models in flipfile

## AI upscaling

flipfile's AI upscaling (Photo → Tools → Upscale 2× / 4×) runs the same four models on
every system: as Core ML models on the Mac, as ONNX models (run by ONNX Runtime) on
Windows and Linux. They are converted from the authors' published weights by
`scripts/upscale-model/convert.py` and `export_onnx.py`, with model code written for
flipfile (`compact.py`, `dat.py`, `restoreformer.py`, `rrdbnet.py`); no code from the
original repositories is included.

| Model file (Mac / Windows and Linux) | Model | Authors | Licence |
|---|---|---|---|
| `upscale.mlmodelc` / `upscale.onnx` | Real-ESRGAN general x4v3 (denoise 0.5) | Xintao Wang et al. | BSD-3-Clause, [`LICENSE-Real-ESRGAN.txt`](LICENSE-Real-ESRGAN.txt) |
| `upscale-large.mlmodelc` / `upscale-large.onnx` | Real-ESRGAN x4plus | Xintao Wang et al. | BSD-3-Clause, [`LICENSE-Real-ESRGAN.txt`](LICENSE-Real-ESRGAN.txt) |
| `faces.mlmodelc` / `faces.onnx` | RestoreFormer++ | Zhouxia Wang, Jiawei Zhang, Tianshui Chen, Wenping Wang, Ping Luo | Apache-2.0, [`LICENSE-RestoreFormerPlusPlus.txt`](LICENSE-RestoreFormerPlusPlus.txt) |
| `faces-upscale.mlmodelc` / `faces-upscale.onnx` | 4xFaceUpSharpLDAT (DAT), a 4x upscaler for faces | Philip Hofmann (Phhofm) | CC-BY-4.0, [`LICENSE-FaceUp.txt`](LICENSE-FaceUp.txt) |
| — / `yunet.onnx` | YuNet face detector (`face_detection_yunet_2026may.onnx`, unchanged) | Shiqi Yu et al. | MIT, [`LICENSE-YuNet.txt`](LICENSE-YuNet.txt) |

Changes from the original works: the weights were converted to Core ML and ONNX; the
compact upscaler is an even blend of `realesr-general-x4v3` and
`realesr-general-wdn-x4v3`; x4plus is stored in float16, RestoreFormer++ and
4xFaceUpSharpLDAT with 8-bit weights. The Mac finds faces with its own Vision framework
instead of YuNet.

- Real-ESRGAN: https://github.com/xinntao/Real-ESRGAN
- RestoreFormer++: https://github.com/wzhouxiff/RestoreFormerPlusPlus
  (Z. Wang et al., "RestoreFormer++: Towards Real-World Blind Face Restoration from
  Undegraded Key-Value Pairs", IEEE TPAMI 2023)
- 4xFaceUpSharpLDAT: https://github.com/Phhofm/models/releases/tag/4xFaceUpDAT_Series
  (network: Z. Chen et al., "Dual Aggregation Transformer for Image Super-Resolution",
  ICCV 2023; trained by Philip Hofmann on his FaceUp dataset, made from FFHQ)
- YuNet: https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet
  (W. Wu, H. Peng, S. Yu, "YuNet: A Tiny Millisecond-level Face Detector", Machine
  Intelligence Research 2023)

### The upscaling engine on Windows and Linux

ONNX Runtime 1.24.4 (Microsoft, MIT licence; `onnxruntime/LICENSE-onnxruntime.txt` and
`ThirdPartyNotices-onnxruntime.txt` next to it in the app). On Windows it runs the models
on the GPU through DirectML, Microsoft's redistributable component for Windows apps
(`onnxruntime/LICENSE-DirectML.txt`). Both come unchanged from Microsoft's official
releases, verified by SHA-256 (`scripts/fetch-onnxruntime.mjs`). ONNX Runtime's
telemetry is switched off.

## Transcribe (speech to text)

Music → Transcribe and Video → Tools → Subtitles (and AI summaries of recordings) write
down what is said with OpenAI's Whisper, on every system.

| Model file | Model | Authors | Licence |
|---|---|---|---|
| `whisper-small.bin` | Whisper `small` (multilingual, 244 M parameters), in whisper.cpp's GGML format with 5-bit weights (`ggml-small-q5_1.bin`, unchanged) | Alec Radford, Jong Wook Kim, Tao Xu, Greg Brockman, Christine McLeavey, Ilya Sutskever (OpenAI) | MIT, [`LICENSE-Whisper.txt`](LICENSE-Whisper.txt) |

- Whisper: https://github.com/openai/whisper (A. Radford et al., "Robust Speech
  Recognition via Large-Scale Weak Supervision", ICML 2023)
- The GGML file: https://huggingface.co/ggerganov/whisper.cpp, verified by SHA-256
  (`scripts/fetch-models.mjs`)

The engine is whisper.cpp with ggml (The ggml authors, MIT licence,
[`LICENSE-whisper.cpp.txt`](LICENSE-whisper.cpp.txt)), compiled into flipfile through the
`whisper-rs` crate (public domain); on Apple silicon it runs on the GPU through Metal.
Sound is decoded by Symphonia (Mozilla Public Licence 2.0, unmodified;
https://github.com/pdeljanov/Symphonia) and, for Opus, by opus1 (BSD-3-Clause).
