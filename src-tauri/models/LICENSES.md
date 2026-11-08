# AI models in flipfile

flipfile's AI upscaling (Photo → Tools → Upscale 2× / 4×) runs the same three models on
every system: as Core ML models on the Mac, as ONNX models (run by ONNX Runtime) on
Windows and Linux. They are converted from the authors' published weights by
`scripts/upscale-model/convert.py` and `export_onnx.py`, with model code written for
flipfile (`compact.py`, `restoreformer.py`, `rrdbnet.py`); no code from the original
repositories is included.

| Model file (Mac / Windows and Linux) | Model | Authors | Licence |
|---|---|---|---|
| `upscale.mlmodelc` / `upscale.onnx` | Real-ESRGAN general x4v3 (denoise 0.5) | Xintao Wang et al. | BSD-3-Clause, [`LICENSE-Real-ESRGAN.txt`](LICENSE-Real-ESRGAN.txt) |
| `upscale-large.mlmodelc` / `upscale-large.onnx` | Real-ESRGAN x4plus | Xintao Wang et al. | BSD-3-Clause, [`LICENSE-Real-ESRGAN.txt`](LICENSE-Real-ESRGAN.txt) |
| `faces.mlmodelc` / `faces.onnx` | RestoreFormer++ | Zhouxia Wang, Jiawei Zhang, Tianshui Chen, Wenping Wang, Ping Luo | Apache-2.0, [`LICENSE-RestoreFormerPlusPlus.txt`](LICENSE-RestoreFormerPlusPlus.txt) |
| — / `yunet.onnx` | YuNet face detector (`face_detection_yunet_2026may.onnx`, unchanged) | Shiqi Yu et al. | MIT, [`LICENSE-YuNet.txt`](LICENSE-YuNet.txt) |

Changes from the original works: the weights were converted to Core ML and ONNX; the
compact upscaler is an even blend of `realesr-general-x4v3` and
`realesr-general-wdn-x4v3`; x4plus is stored in float16 and RestoreFormer++ with 8-bit
weights. The Mac finds faces with its own Vision framework instead of YuNet.

- Real-ESRGAN: https://github.com/xinntao/Real-ESRGAN
- RestoreFormer++: https://github.com/wzhouxiff/RestoreFormerPlusPlus
  (Z. Wang et al., "RestoreFormer++: Towards Real-World Blind Face Restoration from
  Undegraded Key-Value Pairs", IEEE TPAMI 2023)
- YuNet: https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet
  (W. Wu, H. Peng, S. Yu, "YuNet: A Tiny Millisecond-level Face Detector", Machine
  Intelligence Research 2023)

## The engine on Windows and Linux

ONNX Runtime 1.24.4 (Microsoft, MIT licence; `onnxruntime/LICENSE-onnxruntime.txt` and
`ThirdPartyNotices-onnxruntime.txt` next to it in the app). On Windows it runs the models
on the GPU through DirectML, Microsoft's redistributable component for Windows apps
(`onnxruntime/LICENSE-DirectML.txt`). Both come unchanged from Microsoft's official
releases, verified by SHA-256 (`scripts/fetch-onnxruntime.mjs`). ONNX Runtime's
telemetry is switched off.
