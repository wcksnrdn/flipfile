"""RestoreFormer++ (Wang et al., TPAMI 2023), written from scratch for inference only.

The network restores a face aligned to FFHQ's 512 x 512 template: an encoder reads the
degraded face, its features are snapped to a codebook of 1024 high-quality face parts,
and a decoder rebuilds the face while attending back to the encoder's features at 16 and
32 px (so the identity of the input survives). Parameter names follow the released
checkpoint so its weights load unchanged; everything for training (losses,
discriminators, gradient tricks) is left out, and the codebook lookup is an argmin and a
gather, which Core ML runs as is.
"""

import torch
import torch.nn as nn
import torch.nn.functional as F


def norm(channels):
    return nn.GroupNorm(32, channels, eps=1e-6)


def swish(x):
    return x * torch.sigmoid(x)


class ResnetBlock(nn.Module):
    def __init__(self, cin, cout):
        super().__init__()
        self.norm1, self.conv1 = norm(cin), nn.Conv2d(cin, cout, 3, 1, 1)
        self.norm2, self.conv2 = norm(cout), nn.Conv2d(cout, cout, 3, 1, 1)
        self.nin_shortcut = nn.Conv2d(cin, cout, 1) if cin != cout else None

    def forward(self, x):
        h = self.conv1(swish(self.norm1(x)))
        h = self.conv2(swish(self.norm2(h)))
        return (self.nin_shortcut(x) if self.nin_shortcut is not None else x) + h


class Attention(nn.Module):
    """Multi-head attention over a feature map. Keys and values come from `x`; queries
    from `y` when given (cross-attention to the encoder), else from `x` itself."""

    def __init__(self, channels, heads):
        super().__init__()
        # Sizes fixed up front: the network always sees a 512 px face, and Core ML wants
        # shapes it can work out when converting, not from the tensors at run time
        self.heads, self.dim = heads, channels // heads
        self.scale = self.dim ** -0.5
        self.norm1, self.norm2 = norm(channels), norm(channels)
        self.q, self.k, self.v = (nn.Conv2d(channels, channels, 1) for _ in range(3))
        self.proj_out = nn.Conv2d(channels, channels, 1)

    def forward(self, x, y=None):
        hx = self.norm1(x)
        hy = hx if y is None else self.norm2(y)
        split = lambda t: t.reshape(1, self.heads, self.dim, -1).transpose(2, 3)  # 1, heads, hw, dim
        q, k, v = split(self.q(hy)), split(self.k(hx)), split(self.v(hx))
        attn = torch.softmax((q * self.scale) @ k.transpose(2, 3), dim=-1)
        out = (attn @ v).transpose(2, 3).reshape(x.shape)
        return x + self.proj_out(out)


class Downsample(nn.Module):
    def __init__(self, channels):
        super().__init__()
        self.conv = nn.Conv2d(channels, channels, 3, 2, 0)

    def forward(self, x):
        return self.conv(F.pad(x, (0, 1, 0, 1)))


class Upsample(nn.Module):
    def __init__(self, channels):
        super().__init__()
        self.conv = nn.Conv2d(channels, channels, 3, 1, 1)

    def forward(self, x):
        return self.conv(F.interpolate(x, scale_factor=2.0, mode="nearest"))


class Level(nn.Module):
    """One resolution of the encoder or decoder (a holder, for the checkpoint's names)"""


class Middle(nn.Module):
    def __init__(self, channels, heads):
        super().__init__()
        self.block_1 = ResnetBlock(channels, channels)
        self.attn_1 = Attention(channels, heads)
        self.block_2 = ResnetBlock(channels, channels)


CH, MULT, BLOCKS, HEADS, Z = 64, (1, 2, 2, 4, 4, 8), 2, 4, 256
LATENT = 512 // 2 ** (len(MULT) - 1)  # 16


class Encoder(nn.Module):
    def __init__(self):
        super().__init__()
        self.conv_in = nn.Conv2d(3, CH, 3, 1, 1)
        self.down = nn.ModuleList()
        cin, res = CH, 512
        for i, m in enumerate(MULT):
            level = Level()
            level.block = nn.ModuleList()
            level.attn = nn.ModuleList()
            for _ in range(BLOCKS):
                level.block.append(ResnetBlock(cin, CH * m))
                cin = CH * m
                if res == 16:
                    level.attn.append(Attention(cin, HEADS))
            if i != len(MULT) - 1:
                level.downsample = Downsample(cin)
                res //= 2
            self.down.append(level)
        self.mid = Middle(cin, HEADS)
        self.norm_out = norm(cin)
        self.conv_out = nn.Conv2d(cin, Z, 3, 1, 1)

    def forward(self, x):
        """The latent, and the features the decoder attends to: 32 px and 16 px"""
        h = self.conv_in(x)
        skips = {}
        for i, level in enumerate(self.down):
            for j, block in enumerate(level.block):
                h = block(h)
                if len(level.attn):
                    h = level.attn[j](h)
            if i != len(self.down) - 1:
                if i == 4:
                    skips[32] = h
                h = level.downsample(h)
        h = self.mid.block_1(h)
        skips[16] = h
        h = self.mid.attn_1(h)
        h = self.mid.block_2(h)
        mid = h
        return self.conv_out(swish(self.norm_out(h))), skips, mid


class Decoder(nn.Module):
    def __init__(self):
        super().__init__()
        cin, res = CH * MULT[-1], 16
        self.conv_in = nn.Conv2d(Z, cin, 3, 1, 1)
        self.mid = Middle(cin, HEADS)
        levels = []
        for i in reversed(range(len(MULT))):
            level = Level()
            level.block = nn.ModuleList()
            level.attn = nn.ModuleList()
            for _ in range(BLOCKS + 1):
                level.block.append(ResnetBlock(cin, CH * MULT[i]))
                cin = CH * MULT[i]
                if res in (16, 32):
                    level.attn.append(Attention(cin, HEADS))
            if i != 0:
                level.upsample = Upsample(cin)
                res *= 2
            levels.insert(0, level)
        self.up = nn.ModuleList(levels)
        self.norm_out = norm(cin)
        self.conv_out = nn.Conv2d(cin, 3, 3, 1, 1)

    def forward(self, z, skips, mid):
        h = self.conv_in(z)
        h = self.mid.block_1(h)
        h = self.mid.attn_1(h, mid)
        h = self.mid.block_2(h)
        res = 16
        for i in reversed(range(len(self.up))):
            level = self.up[i]
            for j, block in enumerate(level.block):
                h = block(h)
                if len(level.attn):
                    h = level.attn[j](h, skips[res])
            if i != 0:
                h = level.upsample(h)
                res *= 2
        return self.conv_out(swish(self.norm_out(h)))


class Codebook(nn.Module):
    def __init__(self):
        super().__init__()
        self.embedding = nn.Embedding(1024, Z)

    def forward(self, z):
        """Each latent vector (16 x 16 of them) replaced by its nearest codebook entry"""
        flat = z.permute(0, 2, 3, 1).reshape(LATENT * LATENT, Z)
        e = self.embedding.weight
        dist = (flat ** 2).sum(1, keepdim=True) + (e ** 2).sum(1) - 2 * flat @ e.t()
        return e[dist.argmin(1)].reshape(1, LATENT, LATENT, Z).permute(0, 3, 1, 2)


class RestoreFormerPP(nn.Module):
    """A face aligned to the FFHQ template, 1 x 3 x 512 x 512 in [-1, 1] -> restored, same range"""

    def __init__(self):
        super().__init__()
        self.encoder = Encoder()
        self.decoder = Decoder()
        self.quantize = Codebook()
        self.quant_conv = nn.Conv2d(Z, Z, 1)
        self.post_quant_conv = nn.Conv2d(Z, Z, 1)

    def forward(self, x):
        z, skips, mid = self.encoder(x)
        z = self.quantize(self.quant_conv(z))
        return self.decoder(self.post_quant_conv(z), skips, mid).clamp(-1, 1)


def load(path):
    """The released checkpoint, read safely: tensors only, no code runs while loading"""
    state = torch.load(path, map_location="cpu", weights_only=True)["state_dict"]
    weights = {k[len("vqvae."):]: v for k, v in state.items() if k.startswith("vqvae.")}
    net = RestoreFormerPP()
    net.load_state_dict(weights, strict=True)
    return net.eval()
