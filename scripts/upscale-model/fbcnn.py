"""FBCNN (color): removes JPEG artefacts (blocks, ringing, mosquito noise) from a tile,
whatever quality the picture was saved at. Upscale Text runs it before anything else, so
neither the letters nor the paper keep the speckle a JPEG adds around text.

Towards Flexible Blind JPEG Artifacts Removal (Jiang et al., ICCV 2021), Apache-2.0. The
network is the official code, used from a checkout of https://github.com/jiaxi-jiang/FBCNN
at commit 54d1831927506b3247e2d4d245abb4f4dab1a1cd, whose path is given in FBCNN_SRC (it is
only needed to build the model):

    git clone https://github.com/jiaxi-jiang/FBCNN && git -C FBCNN checkout 54d1831
    curl -LO https://github.com/jiaxi-jiang/FBCNN/releases/download/v1.0/fbcnn_color.pth

The network also estimates the JPEG quality, which it uses itself; only the cleaned
picture is returned.
"""

import os
import sys
import types

import torch
from torch import nn


class Cleaned(nn.Module):
    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, x):
        return self.net(x)[0]


def load(path):
    sys.path.insert(0, os.environ.get("FBCNN_SRC", "FBCNN"))
    # The network file imports torchvision without using it
    sys.modules.setdefault("torchvision", types.ModuleType("torchvision"))
    sys.modules.setdefault("torchvision.models", types.ModuleType("torchvision.models"))
    from models.network_fbcnn import FBCNN

    net = FBCNN()
    net.load_state_dict(torch.load(path, map_location="cpu", weights_only=True), strict=True)
    return Cleaned(net.eval()).eval()
