"""Builds the three Core ML models behind flipfile's AI upscaling (src-tauri/models/):

- upscale.mlmodelc (2.4 MB): Real-ESRGAN's compact general model (realesr-general-x4v3),
  blended half and half with its no-denoise twin, Real-ESRGAN's own "denoise strength
  0.5". Fast; used for large pictures. Float16, Neural Engine.
- upscale-large.mlmodelc (33 MB): Real-ESRGAN x4plus (ESRGAN's RRDBNet, rrdbnet.py).
  Far more natural detail on photos; used for small pictures. Float16, Neural Engine.
- faces.mlmodelc (71 MB): RestoreFormer++ (restoreformer.py), redraws a face aligned to a
  512 x 512 template from a dictionary of high-quality face parts while keeping the
  person's identity. 8-bit weights: flipfile runs it on the CPU (on the Neural Engine the
  8-bit model returns a flat grey image).

The two upscalers read a fixed 256 x 256 tile; flipfile cuts images into such tiles
(src-tauri/src/convert/upscale.rs). Licences: Real-ESRGAN BSD-3-Clause (Xintao Wang),
RestoreFormer++ Apache-2.0 (Zhouxia Wang et al.); see src-tauri/models/LICENSES.md.

Weights are read with `weights_only=True`: tensors only, no code in the checkpoints runs.

    python3.11 -m venv venv && ./venv/bin/pip install torch coremltools numpy
    curl -LO https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth
    curl -LO https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-wdn-x4v3.pth
    curl -LO https://github.com/xinntao/Real-ESRGAN/releases/download/v0.1.0/RealESRGAN_x4plus.pth
    curl -L -o RestoreFormerPlusPlus.ckpt https://github.com/wzhouxiff/RestoreFormerPlusPlus/releases/download/v1.0.0/RestoreFormer++.ckpt
    ./venv/bin/python convert.py ../../src-tauri/models
"""

import shutil
import sys
from pathlib import Path

import coremltools as ct
import coremltools.optimize.coreml as cto
import numpy as np
import torch

import compact
import restoreformer
import rrdbnet

TILE = 256


def convert(net, example, name, inputs, outputs, description, licence, quantize=False):
    model = ct.convert(
        torch.jit.trace(net, example),
        inputs=[ct.TensorType(name=inputs, shape=example.shape, dtype=np.float32)],
        outputs=[ct.TensorType(name=outputs, dtype=np.float32)],
        minimum_deployment_target=ct.target.macOS13,
        compute_precision=ct.precision.FLOAT16,
    )
    if quantize:
        config = cto.OptimizationConfig(global_config=cto.OpLinearQuantizerConfig(mode="linear_symmetric"))
        model = cto.linear_quantize_weights(model, config)
    model.short_description = description
    model.license = licence
    return model


def save(model, out_dir: Path, name: str) -> Path:
    """The compiled form, as Xcode would bundle it: what the app loads"""
    package = out_dir / f"{name}.mlpackage"
    model.save(str(package))
    compiled = Path(ct.models.utils.compile_model(str(package)))
    target = out_dir / f"{name}.mlmodelc"
    shutil.rmtree(target, ignore_errors=True)
    shutil.copytree(compiled, target)
    shutil.rmtree(package)
    return target


def check(net, example, target, inputs, outputs, units):
    with torch.no_grad():
        expected = net(example).numpy()
    got = ct.models.CompiledMLModel(str(target), compute_units=units).predict({inputs: example.numpy()})[outputs]
    print(f"{target.name}: mean difference from PyTorch {np.abs(got - expected).mean():.4f}")


def main(out_dir: Path):
    out_dir.mkdir(parents=True, exist_ok=True)
    tile = torch.rand(1, 3, TILE, TILE)
    bsd = "BSD-3-Clause, Copyright (c) 2021 Xintao Wang"

    net = compact.load()
    model = convert(net, tile, "upscale", "image", "upscaled", "Real-ESRGAN general x4v3 (denoise 0.5): 4x of a 256 px tile", bsd)
    check(net, tile, save(model, out_dir, "upscale"), "image", "upscaled", ct.ComputeUnit.ALL)

    net = rrdbnet.load("RealESRGAN_x4plus.pth")
    model = convert(net, tile, "upscale-large", "image", "upscaled", "Real-ESRGAN x4plus: 4x of a 256 px tile", bsd)
    check(net, tile, save(model, out_dir, "upscale-large"), "image", "upscaled", ct.ComputeUnit.ALL)

    net = restoreformer.load("RestoreFormerPlusPlus.ckpt")
    face = torch.rand(1, 3, 512, 512) * 2 - 1
    model = convert(net, face, "faces", "face", "restored",
                    "RestoreFormer++: restores a face aligned to the FFHQ 512 template, [-1, 1] in and out",
                    "Apache-2.0, Copyright (c) 2023 Zhouxia Wang et al.", quantize=True)
    check(net, face, save(model, out_dir, "faces"), "face", "restored", ct.ComputeUnit.CPU_ONLY)


if __name__ == "__main__":
    main(Path(sys.argv[1] if len(sys.argv) > 1 else "."))
