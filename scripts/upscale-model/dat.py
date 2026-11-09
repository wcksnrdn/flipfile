"""DAT, the Dual Aggregation Transformer (Chen et al., ICCV 2023), as the 4xFaceUpSharpLDAT
face upscaler uses it, for loading its released weights. Written for inference on a
fixed tile size: everything that doesn't depend on the picture (the relative position
biases, the masks of shifted windows) is computed once when the weights are loaded, and
cyclic shifts are slices, so the graph converts cleanly to Core ML and ONNX.

The network: a convolution, six residual groups of six blocks (spatial window attention
in rectangular 8 x 32 / 32 x 8 windows and channel attention alternating, each with a
convolution branch and a spatial-gate feed-forward), a convolution, then 4x by two
convolution + pixel-shuffle steps.
"""

import torch
import torch.nn as nn
import torch.nn.functional as F
from safetensors.torch import load_file

DIM = 180
DEPTH = 6
GROUPS = 6
HEADS = 6
SPLIT = (8, 32)
EXPANSION = 4
MEAN = (0.4488, 0.4371, 0.4040)


def tokens(x):
    """(B, C, H, W) -> (B, H*W, C)"""
    return x.flatten(2).transpose(1, 2)


def image(x, h, w):
    """(1, H*W, C) -> (1, C, H, W)"""
    return x.transpose(1, 2).reshape(1, -1, h, w)


def roll(x, dy, dx):
    """torch.roll over the last two dimensions, as slices"""
    if dy:
        x = torch.cat([x[..., -dy:, :], x[..., :-dy, :]], dim=-2)
    if dx:
        x = torch.cat([x[..., :, -dx:], x[..., :, :-dx]], dim=-1)
    return x


class PosBias(nn.Module):
    """Dynamic relative position bias: a small MLP from (dy, dx) to one bias per head"""

    def __init__(self, dim, heads):
        super().__init__()
        p = dim // 4
        self.pos_proj = nn.Linear(2, p)
        self.pos1 = nn.Sequential(nn.LayerNorm(p), nn.ReLU(), nn.Linear(p, p))
        self.pos2 = nn.Sequential(nn.LayerNorm(p), nn.ReLU(), nn.Linear(p, p))
        self.pos3 = nn.Sequential(nn.LayerNorm(p), nn.ReLU(), nn.Linear(p, heads))

    def table(self, hs, ws):
        """(heads, N, N) for an hs x ws window"""
        dy, dx = torch.meshgrid(torch.arange(1 - hs, hs), torch.arange(1 - ws, ws), indexing="ij")
        offsets = torch.stack([dy.flatten(), dx.flatten()], 1).float()
        bias = self.pos3(self.pos2(self.pos1(self.pos_proj(offsets))))
        ys, xs = torch.meshgrid(torch.arange(hs), torch.arange(ws), indexing="ij")
        ys, xs = ys.flatten(), xs.flatten()
        index = (ys[:, None] - ys[None, :] + hs - 1) * (2 * ws - 1) + (xs[:, None] - xs[None, :] + ws - 1)
        return bias[index].permute(2, 0, 1).contiguous()


class Branch(nn.Module):
    """Self-attention within hs x ws windows, on half the channels"""

    def __init__(self, dim, heads, hs, ws):
        super().__init__()
        self.dim, self.heads, self.hs, self.ws = dim, heads, hs, ws
        self.scale = (dim // heads) ** -0.5
        self.pos = PosBias(dim // 4, heads)

    def finish(self, size, shift):
        self.size = size
        self.register_buffer("bias", self.pos.table(self.hs, self.ws), persistent=False)
        labels = None
        if shift:
            # Each pixel's region after the cyclic shift; pixels of different regions in
            # one window don't attend to each other
            label = torch.zeros(size, size)
            n = 0
            for ys in (slice(0, -self.hs), slice(-self.hs, -shift[0]), slice(-shift[0], None)):
                for xs in (slice(0, -self.ws), slice(-self.ws, -shift[1]), slice(-shift[1], None)):
                    label[ys, xs] = n
                    n += 1
            # Kept small (windows x N); the N x N mask is made from it when run
            labels = self.windows(label[None, None], 1).transpose(1, 2)  # nW, 1, N
        self.register_buffer("labels", labels, persistent=False)
        del self.pos

    def windows(self, x, c):
        """(1, c, size, size) -> (windows, hs * ws, c)"""
        x = x.reshape(1, c, self.size // self.hs, self.hs, self.size // self.ws, self.ws)
        return x.permute(0, 2, 4, 3, 5, 1).reshape(-1, self.hs * self.ws, c)

    def forward(self, q, k, v):
        """q, k, v: (1, C', H, W) -> (1, C', H, W)"""
        b, c, h, w = 1, self.dim, self.size, self.size
        n, hd = self.hs * self.ws, c // self.heads
        split = lambda t: self.windows(t, c).reshape(-1, n, self.heads, hd).permute(0, 2, 1, 3)
        q, k, v = split(q), split(k), split(v)
        attn = (q * self.scale) @ k.transpose(-2, -1) + self.bias
        if self.labels is not None:
            mask = (self.labels != self.labels.transpose(1, 2)).float() * -100.0  # nW, N, N
            attn = attn + mask.unsqueeze(1)
        x = attn.softmax(-1) @ v  # nW, heads, N, hd
        x = x.transpose(1, 2).reshape(b, h // self.hs, w // self.ws, self.hs, self.ws, c)
        return x.permute(0, 5, 1, 3, 2, 4).reshape(b, c, h, w)


def interactions(dim):
    channel = nn.Sequential(
        nn.AdaptiveAvgPool2d(1), nn.Conv2d(dim, dim // 8, 1), nn.BatchNorm2d(dim // 8), nn.GELU(), nn.Conv2d(dim // 8, dim, 1)
    )
    spatial = nn.Sequential(nn.Conv2d(dim, dim // 16, 1), nn.BatchNorm2d(dim // 16), nn.GELU(), nn.Conv2d(dim // 16, 1, 1))
    return channel, spatial


class SpatialAttention(nn.Module):
    """Two window branches (8 x 32 and 32 x 8) on half the channels each, a depthwise
    convolution branch, and the interaction between them"""

    def __init__(self, dim, heads, shifted):
        super().__init__()
        self.shifted = shifted
        self.qkv = nn.Linear(dim, dim * 3)
        self.proj = nn.Linear(dim, dim)
        self.attns = nn.ModuleList([Branch(dim // 2, heads // 2, *SPLIT), Branch(dim // 2, heads // 2, *SPLIT[::-1])])
        self.dwconv = nn.Sequential(nn.Conv2d(dim, dim, 3, 1, 1, groups=dim), nn.BatchNorm2d(dim), nn.GELU())
        self.channel_interaction, self.spatial_interaction = interactions(dim)

    def finish(self, size):
        s0, s1 = SPLIT[0] // 2, SPLIT[1] // 2
        self.shifts = ((s0, s1), (s1, s0)) if self.shifted else (None, None)
        for branch, shift in zip(self.attns, self.shifts):
            branch.finish(size, shift)

    def forward(self, x, h, w):
        c = DIM
        q, k, v = (image(t, h, w) for t in self.qkv(x).chunk(3, dim=-1))
        halves = []
        for i, (branch, shift) in enumerate(zip(self.attns, self.shifts)):
            part = [t[:, i * c // 2:(i + 1) * c // 2] for t in (q, k, v)]
            if shift:
                part = [roll(t, h - shift[0], w - shift[1]) for t in part]
            out = branch(*part)
            if shift:
                out = roll(out, shift[0], shift[1])
            halves.append(out)
        attended = torch.cat(halves, 1)  # B, C, H, W
        conv = self.dwconv(v)
        # Each branch weighs the other: per channel from the convolution, per pixel from the attention
        channel, spatial = self.channel_interaction(conv), self.spatial_interaction(attended)
        return self.proj(tokens(attended * torch.sigmoid(channel) + conv * torch.sigmoid(spatial)))


class ChannelAttention(nn.Module):
    """Attention between channels, with a depthwise convolution branch and their interaction"""

    def __init__(self, dim, heads):
        super().__init__()
        self.heads = heads
        self.temperature = nn.Parameter(torch.ones(heads, 1, 1))
        self.qkv = nn.Linear(dim, dim * 3)
        self.proj = nn.Linear(dim, dim)
        self.dwconv = nn.Sequential(nn.Conv2d(dim, dim, 3, 1, 1, groups=dim), nn.BatchNorm2d(dim), nn.GELU())
        self.channel_interaction, self.spatial_interaction = interactions(dim)

    def finish(self, size):
        pass

    def forward(self, x, h, w):
        b, n, c = 1, h * w, DIM
        hd = c // self.heads
        q, k, v = (t.transpose(1, 2).reshape(b, self.heads, hd, n) for t in self.qkv(x).chunk(3, dim=-1))
        v_img = v.reshape(b, c, h, w)
        q = q / q.norm(dim=-1, keepdim=True).clamp_min(1e-12)
        k = k / k.norm(dim=-1, keepdim=True).clamp_min(1e-12)
        attn = ((q @ k.transpose(-2, -1)) * self.temperature).softmax(-1)
        attended = (attn @ v).reshape(b, c, h, w)
        conv = self.dwconv(v_img)
        # Here the other way round: per pixel from the convolution, per channel from the attention
        spatial, channel = self.spatial_interaction(conv), self.channel_interaction(attended)
        return self.proj(tokens(attended * torch.sigmoid(spatial) + conv * torch.sigmoid(channel)))


class SpatialGate(nn.Module):
    def __init__(self, dim):
        super().__init__()
        self.norm = nn.LayerNorm(dim)
        self.conv = nn.Conv2d(dim, dim, 3, 1, 1, groups=dim)

    def forward(self, x, h, w):
        x1, x2 = x.chunk(2, dim=-1)
        return x1 * tokens(self.conv(image(self.norm(x2), h, w)))


class FeedForward(nn.Module):
    def __init__(self, dim, hidden):
        super().__init__()
        self.fc1 = nn.Linear(dim, hidden)
        self.act = nn.GELU()
        self.sg = SpatialGate(hidden // 2)
        self.fc2 = nn.Linear(hidden // 2, dim)

    def forward(self, x, h, w):
        return self.fc2(self.sg(self.act(self.fc1(x)), h, w))


class Block(nn.Module):
    def __init__(self, group, index):
        super().__init__()
        self.norm1 = nn.LayerNorm(DIM)
        self.norm2 = nn.LayerNorm(DIM)
        if index % 2 == 0:
            # Shifted windows in blocks 2 of even groups, 0 and 4 of odd ones
            shifted = (group % 2 == 0 and index > 0 and (index - 2) % 4 == 0) or (group % 2 == 1 and index % 4 == 0)
            self.attn = SpatialAttention(DIM, HEADS, shifted)
        else:
            self.attn = ChannelAttention(DIM, HEADS)
        self.ffn = FeedForward(DIM, DIM * EXPANSION)

    def forward(self, x, h, w):
        x = x + self.attn(self.norm1(x), h, w)
        return x + self.ffn(self.norm2(x), h, w)


class Group(nn.Module):
    def __init__(self, group):
        super().__init__()
        self.blocks = nn.ModuleList([Block(group, i) for i in range(DEPTH)])
        self.conv = nn.Conv2d(DIM, DIM, 3, 1, 1)

    def forward(self, x, h, w):
        res = x
        for block in self.blocks:
            x = block(x, h, w)
        return res + tokens(self.conv(image(x, h, w)))


class DAT(nn.Module):
    def __init__(self):
        super().__init__()
        self.conv_first = nn.Conv2d(3, DIM, 3, 1, 1)
        self.before_RG = nn.Sequential(nn.Identity(), nn.LayerNorm(DIM))
        self.layers = nn.ModuleList([Group(g) for g in range(GROUPS)])
        self.norm = nn.LayerNorm(DIM)
        self.conv_after_body = nn.Conv2d(DIM, DIM, 3, 1, 1)
        self.conv_before_upsample = nn.Sequential(nn.Conv2d(DIM, 64, 3, 1, 1), nn.LeakyReLU())
        self.upsample = nn.Sequential(nn.Conv2d(64, 256, 3, 1, 1), nn.PixelShuffle(2), nn.Conv2d(64, 256, 3, 1, 1), nn.PixelShuffle(2))
        self.conv_last = nn.Conv2d(64, 3, 3, 1, 1)
        self.register_buffer("mean", torch.tensor(MEAN).view(1, 3, 1, 1), persistent=False)

    def finish(self, size):
        """Fixes the tile size the network reads (a multiple of 32)"""
        assert size % max(SPLIT) == 0
        self.size = size
        with torch.no_grad():
            for group in self.layers:
                for block in group.blocks:
                    block.attn.finish(size)
        return self

    def forward(self, x):
        h = w = self.size
        x = self.conv_first(x - self.mean)
        f = self.before_RG(tokens(x))
        for group in self.layers:
            f = group(f, h, w)
        x = self.conv_after_body(image(self.norm(f), h, w)) + x
        x = self.conv_last(self.upsample(self.conv_before_upsample(x)))
        return x + self.mean


def load(path, size):
    """The released safetensors weights (tensors only), for size x size tiles"""
    state = load_file(path)
    net = DAT()
    own = net.state_dict()
    # Buffers the release carries for its own code (index tables, masks) are not needed:
    # everything else must match exactly
    skip = ("relative_position_index", "rpe_biases", "attn_mask_0", "attn_mask_1")
    weights = {k: v for k, v in state.items() if not k.endswith(skip)}
    missing = set(own) - set(weights)
    unexpected = set(weights) - set(own)
    assert not missing and not unexpected, (sorted(missing)[:5], sorted(unexpected)[:5])
    net.load_state_dict(weights, strict=True)
    return net.eval().finish(size)
