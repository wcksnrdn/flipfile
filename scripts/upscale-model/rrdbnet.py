"""ESRGAN's RRDBNet (Wang et al., 2018) as Real-ESRGAN's x4plus model uses it, for loading
its released weights: 23 residual-in-residual dense blocks on the small image, then two
nearest-neighbour 2x steps with convolutions. Written for inference only."""

import torch
import torch.nn as nn
import torch.nn.functional as F


class DenseBlock(nn.Module):
    """Five convolutions, each seeing every earlier output; scaled residual"""

    def __init__(self, feat=64, grow=32):
        super().__init__()
        self.conv1 = nn.Conv2d(feat, grow, 3, 1, 1)
        self.conv2 = nn.Conv2d(feat + grow, grow, 3, 1, 1)
        self.conv3 = nn.Conv2d(feat + 2 * grow, grow, 3, 1, 1)
        self.conv4 = nn.Conv2d(feat + 3 * grow, grow, 3, 1, 1)
        self.conv5 = nn.Conv2d(feat + 4 * grow, feat, 3, 1, 1)

    def forward(self, x):
        act = lambda t: F.leaky_relu(t, 0.2)
        x1 = act(self.conv1(x))
        x2 = act(self.conv2(torch.cat((x, x1), 1)))
        x3 = act(self.conv3(torch.cat((x, x1, x2), 1)))
        x4 = act(self.conv4(torch.cat((x, x1, x2, x3), 1)))
        x5 = self.conv5(torch.cat((x, x1, x2, x3, x4), 1))
        return x5 * 0.2 + x


class RRDB(nn.Module):
    def __init__(self, feat=64, grow=32):
        super().__init__()
        self.rdb1, self.rdb2, self.rdb3 = DenseBlock(feat, grow), DenseBlock(feat, grow), DenseBlock(feat, grow)

    def forward(self, x):
        return self.rdb3(self.rdb2(self.rdb1(x))) * 0.2 + x


class RRDBNet(nn.Module):
    """1 x 3 x H x W in [0, 1] -> 1 x 3 x 4H x 4W"""

    def __init__(self, feat=64, blocks=23, grow=32):
        super().__init__()
        self.conv_first = nn.Conv2d(3, feat, 3, 1, 1)
        self.body = nn.Sequential(*[RRDB(feat, grow) for _ in range(blocks)])
        self.conv_body = nn.Conv2d(feat, feat, 3, 1, 1)
        self.conv_up1 = nn.Conv2d(feat, feat, 3, 1, 1)
        self.conv_up2 = nn.Conv2d(feat, feat, 3, 1, 1)
        self.conv_hr = nn.Conv2d(feat, feat, 3, 1, 1)
        self.conv_last = nn.Conv2d(feat, 3, 3, 1, 1)

    def forward(self, x):
        act = lambda t: F.leaky_relu(t, 0.2)
        feat = self.conv_first(x)
        feat = feat + self.conv_body(self.body(feat))
        feat = act(self.conv_up1(F.interpolate(feat, scale_factor=2.0, mode="nearest")))
        feat = act(self.conv_up2(F.interpolate(feat, scale_factor=2.0, mode="nearest")))
        return self.conv_last(act(self.conv_hr(feat)))


def load(path):
    """The released weights, read safely (tensors only)"""
    state = torch.load(path, map_location="cpu", weights_only=True)
    state = state.get("params_ema", state.get("params", state))
    net = RRDBNet()
    net.load_state_dict(state, strict=True)
    return net.eval()
