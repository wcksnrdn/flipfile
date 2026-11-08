"""Builds the ONNX models behind flipfile's AI upscaling on Windows and Linux, where ONNX
Runtime runs them (src-tauri/src/platform/onnx.rs). The same networks and weights as the
Core ML models of macOS (convert.py), so the results match:

- upscale.onnx (2.4 MB): Real-ESRGAN's compact general model, denoise 0.5 (compact.py).
- upscale-large.onnx (33 MB): Real-ESRGAN x4plus (rrdbnet.py).
- faces.onnx (71 MB): RestoreFormer++ (restoreformer.py).
- yunet.onnx (0.2 MB): YuNet, the face detector (MIT, Shiqi Yu; OpenCV Zoo's
  face_detection_yunet_2026may.onnx, copied as it is). macOS uses Vision instead.

Every model reads and writes planar RGB floats at a fixed size, as the Core ML ones do.
Weights are stored smaller than PyTorch's float32 and widened back when a model is
loaded or run, so all arithmetic stays float32 (ONNX Runtime's CPU engine is slow in
float16): the upscalers as float16 (one Cast each), the face model as 8-bit integers
with one scale per output channel (one DequantizeLinear each), as on macOS.

Weights are read with `weights_only=True`: tensors only, no code in the checkpoints runs.

    python3.11 -m venv venv && ./venv/bin/pip install torch onnx onnxruntime numpy
    (download the four checkpoints as in convert.py, and face_detection_yunet_2026may.onnx from
    https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)
    ./venv/bin/python export_onnx.py ../../src-tauri/models
"""

import shutil
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
import torch
from onnx import helper, numpy_helper

import compact
import restoreformer
import rrdbnet

TILE = 256
FACE = 512
# Smaller tensors (biases, norms) stay float32: they hardly count
SMALL = 4096


def export(net, example, path: Path, inputs: str, outputs: str):
    with torch.no_grad():
        torch.onnx.export(net, example, str(path), input_names=[inputs], output_names=[outputs], opset_version=17, dynamo=False)


def shrink(path: Path, mode: str):
    """Stores the large float32 weights as float16 ("half") or per-channel int8 ("int8"),
    each widened back to float32 by a node in front of its first use"""
    model = onnx.load(str(path))
    graph = model.graph
    kept, added = [], []
    for tensor in graph.initializer:
        values = numpy_helper.to_array(tensor)
        if values.dtype != np.float32 or values.size < SMALL or values.ndim < 2:
            kept.append(tensor)
            continue
        name = tensor.name
        if mode == "half":
            kept.append(numpy_helper.from_array(values.astype(np.float16), f"{name}_half"))
            added.append(helper.make_node("Cast", [f"{name}_half"], [name], to=onnx.TensorProto.FLOAT))
        else:
            # Symmetric, one scale per slice along the first axis
            flat = values.reshape(values.shape[0], -1)
            scale = np.maximum(np.abs(flat).max(axis=1), 1e-12) / 127.0
            q = np.clip(np.round(flat / scale[:, None]), -127, 127).astype(np.int8).reshape(values.shape)
            kept += [
                numpy_helper.from_array(q, f"{name}_q"),
                numpy_helper.from_array(scale.astype(np.float32), f"{name}_scale"),
                numpy_helper.from_array(np.zeros(values.shape[0], np.int8), f"{name}_zero"),
            ]
            added.append(helper.make_node("DequantizeLinear", [f"{name}_q", f"{name}_scale", f"{name}_zero"], [name], axis=0))
    del graph.initializer[:]
    graph.initializer.extend(kept)
    nodes = added + list(graph.node)
    del graph.node[:]
    graph.node.extend(nodes)
    onnx.checker.check_model(model)
    onnx.save(model, str(path))


def describe(path: Path, description: str, licence: str):
    model = onnx.load(str(path))
    model.doc_string = f"{description}. Licence: {licence}"
    onnx.save(model, str(path))


def check(net, example, path: Path, inputs: str, outputs: str):
    with torch.no_grad():
        expected = net(example).numpy()
    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    got = session.run([outputs], {inputs: example.numpy()})[0]
    print(f"{path.name} ({path.stat().st_size / 1e6:.1f} MB): mean difference from PyTorch {np.abs(got - expected).mean():.4f}")


def main(out_dir: Path, weights: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    tile = torch.rand(1, 3, TILE, TILE)
    bsd = "BSD-3-Clause, Copyright (c) 2021 Xintao Wang"

    net = compact.load()
    path = out_dir / "upscale.onnx"
    export(net, tile, path, "image", "upscaled")
    shrink(path, "half")
    describe(path, "Real-ESRGAN general x4v3 (denoise 0.5): 4x of a 256 px tile", bsd)
    check(net, tile, path, "image", "upscaled")

    net = rrdbnet.load("RealESRGAN_x4plus.pth")
    path = out_dir / "upscale-large.onnx"
    export(net, tile, path, "image", "upscaled")
    shrink(path, "half")
    describe(path, "Real-ESRGAN x4plus: 4x of a 256 px tile", bsd)
    check(net, tile, path, "image", "upscaled")

    net = restoreformer.load("RestoreFormerPlusPlus.ckpt")
    face = torch.rand(1, 3, FACE, FACE) * 2 - 1
    path = out_dir / "faces.onnx"
    export(net, face, path, "face", "restored")
    shrink(path, "int8")
    describe(path, "RestoreFormer++: restores a face aligned to the FFHQ 512 template, [-1, 1] in and out",
             "Apache-2.0, Copyright (c) 2023 Zhouxia Wang et al.")
    check(net, face, path, "face", "restored")

    shutil.copyfile(weights / "face_detection_yunet_2026may.onnx", out_dir / "yunet.onnx")


if __name__ == "__main__":
    main(Path(sys.argv[1] if len(sys.argv) > 1 else "."), Path("."))
