"""Builds the ONNX models behind flipfile's AI upscaling on Windows and Linux, where ONNX
Runtime runs them (src-tauri/src/platform/onnx.rs). The same networks and weights as the
Core ML models of macOS (convert.py), so the results match:

- upscale.onnx (2.4 MB): Real-ESRGAN's compact general model, denoise 0.5 (compact.py).
- upscale-photo.onnx (33 MB): x4plus 70% + 4xNomos8kSC 30% (rrdbnet.py), Upscale Photo's and Plain's.
- upscale-text.onnx (33 MB): RealESRNet x4plus (rrdbnet.py), Upscale Text's shapes.
- dejpeg.onnx (144 MB): FBCNN (fbcnn.py), JPEG artefacts removed before Upscale Text.
- colorize.onnx (147 MB): DDColor tiny (ddcolor.py), colours black-and-white photos.
- faces.onnx (71 MB): RestoreFormer++ (restoreformer.py).
- faces-upscale.onnx (33 MB): 4xFaceUpSharpLDAT (dat.py), the face upscaler, on 128 px tiles.
- yunet.onnx (0.2 MB): YuNet, the face detector (MIT, Shiqi Yu; OpenCV Zoo's
  face_detection_yunet_2026may.onnx, copied as it is). macOS uses Vision instead.

Every model reads and writes planar RGB floats at a fixed size, as the Core ML ones do.
Weights are stored smaller than PyTorch's float32 and widened back when a model is
loaded or run, so all arithmetic stays float32 (ONNX Runtime's CPU engine is slow in
float16): the upscalers, dejpeg and colorize as float16 (one Cast each), the face model as 8-bit integers
with one scale per output channel (one DequantizeLinear each), as on macOS; so is the
face upscaler.

Weights are read with `weights_only=True`: tensors only, no code in the checkpoints runs.

    python3.11 -m venv venv && ./venv/bin/pip install torch==2.7.0 onnx onnxruntime numpy safetensors
    (download the checkpoints as in convert.py, and face_detection_yunet_2026may.onnx from
    https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)
    ./venv/bin/python export_onnx.py ../../src-tauri/models [model names; all by default]
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
import dat
import ddcolor
import fbcnn
import restoreformer
import rrdbnet

TILE = 256
# Upscale Photo's upscaler: x4plus's crisp detail, with 4xNomos8kSC's (x4plus trained
# further on real photos) more natural texture and calmer JPEG handling
PHOTO = [("RealESRGAN_x4plus.pth", 0.7), ("4xNomos8kSC.safetensors", 0.3)]
FACE = 512
FACE_TILE = 128
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
    # Large constants inside the graph (tables computed when the weights were loaded)
    # become weights too, so they are stored smaller like the others
    nodes = []
    for node in graph.node:
        if node.op_type == "Constant" and node.attribute[0].type == onnx.AttributeProto.TENSOR:
            values = numpy_helper.to_array(node.attribute[0].t)
            if values.dtype == np.float32 and values.size >= SMALL:
                graph.initializer.append(numpy_helper.from_array(values, node.output[0]))
                continue
        nodes.append(node)
    del graph.node[:]
    graph.node.extend(nodes)
    # Identical tensors (the face upscaler's window masks, one per shifted block) are stored once
    first, same = {}, {}
    for tensor in graph.initializer:
        key = (tensor.data_type, tuple(tensor.dims), numpy_helper.to_array(tensor).tobytes())
        if key in first:
            same[tensor.name] = first[key]
        else:
            first[key] = tensor.name
    if same:
        unique = [t for t in graph.initializer if t.name not in same]
        del graph.initializer[:]
        graph.initializer.extend(unique)
        for node in graph.node:
            for i, name in enumerate(node.input):
                if name in same:
                    node.input[i] = same[name]
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


def main(out_dir: Path, weights: Path, only):
    out_dir.mkdir(parents=True, exist_ok=True)
    tile = torch.rand(1, 3, TILE, TILE)
    bsd = "BSD-3-Clause, Copyright (c) 2021 Xintao Wang"
    wanted = lambda name: not only or name in only

    def build(name, net, example, inputs, outputs, mode, description, licence):
        path = out_dir / f"{name}.onnx"
        export(net, example, path, inputs, outputs)
        shrink(path, mode)
        describe(path, description, licence)
        check(net, example, path, inputs, outputs)

    if wanted("faces-upscale"):
        build("faces-upscale", dat.load("4xFaceUpSharpLDAT.safetensors", FACE_TILE), torch.rand(1, 3, FACE_TILE, FACE_TILE),
              "image", "upscaled", "int8", "4xFaceUpSharpLDAT (DAT): 4x of a 128 px tile of a face",
              "CC-BY-4.0, 4xFaceUpSharpLDAT by Philip Hofmann (Phhofm)")
    if wanted("upscale"):
        build("upscale", compact.load(), tile, "image", "upscaled", "half",
              "Real-ESRGAN general x4v3 (denoise 0.5): 4x of a 256 px tile", bsd)
    if wanted("upscale-photo"):
        build("upscale-photo", rrdbnet.blend(PHOTO), tile, "image", "upscaled", "half",
              "Real-ESRGAN x4plus 70% + 4xNomos8kSC 30% (RRDBNet): 4x of a 256 px tile of a photo", "BSD-3-Clause, Real-ESRGAN by Xintao Wang; CC-BY-4.0, 4xNomos8kSC by Philip Hofmann (Phhofm)")
    if wanted("upscale-text"):
        build("upscale-text", rrdbnet.load("RealESRNet_x4plus.pth"), tile, "image", "upscaled", "half",
              "RealESRNet x4plus: 4x of a 256 px tile", bsd)
    if wanted("dejpeg"):
        build("dejpeg", fbcnn.load("fbcnn_color.pth"), tile, "image", "cleaned", "half",
              "FBCNN: JPEG artefacts removed from a 256 px tile", "Apache-2.0, FBCNN by Jiaxi Jiang, Kai Zhang and Radu Timofte")
    if wanted("colorize"):
        grey = torch.rand(1, 1, 512, 512).expand(1, 3, 512, 512).contiguous()
        build("colorize", ddcolor.load("ddcolor_paper_tiny.pth"), grey, "image", "ab", "half",
              "DDColor tiny: the Lab a and b of a grey 512 x 512 photo (0 to 1 in, Lab units out)",
              "Apache-2.0, DDColor by Xiaoyang Kang et al.")
    if wanted("faces"):
        build("faces", restoreformer.load("RestoreFormerPlusPlus.ckpt"), torch.rand(1, 3, FACE, FACE) * 2 - 1, "face", "restored",
              "int8", "RestoreFormer++: restores a face aligned to the FFHQ 512 template, [-1, 1] in and out",
              "Apache-2.0, Copyright (c) 2023 Zhouxia Wang et al.")
    if wanted("yunet"):
        shutil.copyfile(weights / "face_detection_yunet_2026may.onnx", out_dir / "yunet.onnx")


if __name__ == "__main__":
    main(Path(sys.argv[1] if len(sys.argv) > 1 else "."), Path("."), set(sys.argv[2:]))
