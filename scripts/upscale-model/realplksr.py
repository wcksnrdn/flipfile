"""RealPLKSR with a DySample upsampler, as 4xNomos2_realplksr_dysample uses it: flipfile's
detailed photo upscaler (texture: skin, fabric, foliage; trained with JPEG compression).

Partial Large Kernel CNNs for Efficient Super-Resolution, https://arxiv.org/abs/2404.11848;
DySample from "Learning to Upsample by Learning to Sample", https://arxiv.org/abs/2308.15085.
Written for inference after spandrel's implementation (MIT, chaiNNer-org); the weights
are 4xNomos2_realplksr_dysample by Philip Hofmann (Phhofm), CC-BY-4.0.

The network reads a fixed tile: its position tables are built for that size, as
constants, so the traced graph holds no shape arithmetic (Core ML can't convert it).
"""

import torch
import torch.nn.functional as F
from safetensors.torch import load_file
from torch import nn


class DCCM(nn.Sequential):
    """Doubled convolutional channel mixer"""

    def __init__(self, dim):
        super().__init__(nn.Conv2d(dim, dim * 2, 3, 1, 1), nn.Mish(), nn.Conv2d(dim * 2, dim, 3, 1, 1))


class PLKConv2d(nn.Module):
    """A large kernel on the first `idx` channels only"""

    def __init__(self, dim, kernel_size):
        super().__init__()
        self.conv = nn.Conv2d(dim, dim, kernel_size, 1, kernel_size // 2)
        self.idx = dim

    def forward(self, x):
        # Split and joined again (not written in place, which tracing turns into copies)
        return torch.cat([self.conv(x[:, : self.idx]), x[:, self.idx :]], dim=1)


class EA(nn.Module):
    """Element-wise attention"""

    def __init__(self, dim):
        super().__init__()
        self.f = nn.Sequential(nn.Conv2d(dim, dim, 3, 1, 1), nn.Sigmoid())

    def forward(self, x):
        return x * self.f(x)


class PLKBlock(nn.Module):
    def __init__(self, dim, kernel_size, split_ratio, norm_groups):
        super().__init__()
        self.channel_mixer = DCCM(dim)
        self.lk = PLKConv2d(int(dim * split_ratio), kernel_size)
        self.attn = EA(dim)
        self.refine = nn.Conv2d(dim, dim, 1, 1, 0)
        self.norm = nn.GroupNorm(norm_groups, dim)

    def forward(self, x):
        return self.norm(self.refine(self.attn(self.lk(self.channel_mixer(x))))) + x


class DySample(nn.Module):
    """Learned upsampling: each output pixel samples the features at a predicted offset"""

    def __init__(self, in_channels, out_ch, scale, groups, tile):
        super().__init__()
        self.scale, self.groups = scale, groups
        self.end_conv = nn.Conv2d(in_channels, out_ch, kernel_size=1)
        self.offset = nn.Conv2d(in_channels, 2 * groups * scale**2, 1)
        self.scope = nn.Conv2d(in_channels, 2 * groups * scale**2, 1, bias=False)
        h = torch.arange((-scale + 1) / 2, (scale - 1) / 2 + 1) / scale
        init = torch.stack(torch.meshgrid([h, h], indexing="ij")).transpose(1, 2).repeat(1, groups, 1).reshape(1, -1, 1, 1)
        self.register_buffer("init_pos", init)
        # Every pixel's centre, and the scale that maps the tile to -1..1: constants for
        # the tile's fixed size
        self.tile = tile
        grid = torch.stack(torch.meshgrid([torch.arange(tile) + 0.5, torch.arange(tile) + 0.5], indexing="ij"))
        self.register_buffer("grid", grid.transpose(1, 2).unsqueeze(1).unsqueeze(0).float())
        self.register_buffer("normalizer", torch.tensor([tile, tile], dtype=torch.float32).view(1, 2, 1, 1, 1))

    def forward(self, x):
        b, h, w = 1, self.tile, self.tile
        offset = (self.offset(x) * self.scope(x).sigmoid() * 0.5 + self.init_pos).view(b, 2, -1, h, w)
        coords = 2 * (self.grid + offset) / self.normalizer - 1
        coords = (
            F.pixel_shuffle(coords.reshape(b, -1, h, w), self.scale)
            .view(b, 2, -1, self.scale * h, self.scale * w)
            .permute(0, 2, 3, 4, 1)
            .contiguous()
            .flatten(0, 1)
        )
        out = F.grid_sample(
            x.reshape(b * self.groups, -1, h, w), coords, mode="bilinear", align_corners=False, padding_mode="border"
        ).view(b, -1, self.scale * h, self.scale * w)
        return self.end_conv(out)


class RealPLKSR(nn.Module):
    def __init__(self, tile, dim=64, n_blocks=28, scale=4, kernel_size=17, split_ratio=0.25, norm_groups=4):
        super().__init__()
        self.scale = scale
        self.feats = nn.Sequential(
            nn.Conv2d(3, dim, 3, 1, 1),
            *[PLKBlock(dim, kernel_size, split_ratio, norm_groups) for _ in range(n_blocks)],
            nn.Dropout2d(0),
            nn.Conv2d(dim, 3 * scale**2, 3, 1, 1),
        )
        self.to_img = DySample(3 * scale**2, 3, scale, groups=4, tile=tile)

    def forward(self, x):
        x = self.feats(x) + torch.repeat_interleave(x, self.scale**2, dim=1)
        return self.to_img(x)


def load(path, tile):
    net = RealPLKSR(tile)
    state = load_file(path)
    net.load_state_dict(state, strict=False)  # the position tables are built, not loaded
    missing = set(net.state_dict()) - set(state)
    assert missing == {"to_img.grid", "to_img.normalizer"}, missing
    return net.eval()
