"""Real-ESRGAN's compact general model (realesr-general-x4v3), blended half and half with
its no-denoise twin: Real-ESRGAN's own "denoise strength 0.5". Written for inference only."""

import torch
import torch.nn as nn
import torch.nn.functional as F

DENOISE = 0.5


class SRVGGNetCompact(nn.Module):
    """Real-ESRGAN's compact network (realesrgan/archs/srvgg_arch.py): 32 convolutions on
    the small image, then a pixel shuffle to 4x, added to a nearest-neighbour enlargement."""

    def __init__(self, feat=64, conv=32, scale=4):
        super().__init__()
        self.scale = scale
        body = [nn.Conv2d(3, feat, 3, 1, 1), nn.PReLU(feat)]
        for _ in range(conv):
            body += [nn.Conv2d(feat, feat, 3, 1, 1), nn.PReLU(feat)]
        body.append(nn.Conv2d(feat, 3 * scale * scale, 3, 1, 1))
        self.body = nn.ModuleList(body)
        self.up = nn.PixelShuffle(scale)

    def forward(self, x):
        out = x
        for layer in self.body:
            out = layer(out)
        return self.up(out) + F.interpolate(x, scale_factor=self.scale, mode="nearest")


def weights(path):
    state = torch.load(path, map_location="cpu", weights_only=True)
    return state.get("params", state)


def load():
    sharp, soft = weights("realesr-general-x4v3.pth"), weights("realesr-general-wdn-x4v3.pth")
    net = SRVGGNetCompact()
    net.load_state_dict({k: DENOISE * sharp[k] + (1 - DENOISE) * soft[k] for k in sharp})
    return net.eval()
