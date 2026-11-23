"""DDColor (tiny): colours a black-and-white photo. Reads its lightness as a grey 512 x 512
RGB picture (values 0 to 1) and returns the two Lab colour channels (a, b) at 512 x 512;
flipfile scales them to the photo's size and keeps the photo's own lightness
(src-tauri/src/convert/colour.rs).

DDColor: Towards Photo-Realistic Image Colorization via Dual Decoders (Kang et al.,
ICCV 2023), Apache-2.0. The network is the official code, used from a checkout of
https://github.com/piddnad/DDColor at commit 2adb63f2656ac41cbdf7b894cddd94121a3faf13,
whose path is given in DDCOLOR_SRC (it is only needed to build the model):

    git clone https://github.com/piddnad/DDColor && git -C DDColor checkout 2adb63f
    curl -L -o ddcolor_paper_tiny.pth https://huggingface.co/piddnad/ddcolor_paper_tiny/resolve/main/pytorch_model.bin
"""

import os
import sys
import types

import torch


def _network():
    """The DDColor class, loaded from its own files: the packages around it would pull in
    OpenCV, torchvision and more, none of which the network needs"""
    src = os.path.join(os.environ.get("DDCOLOR_SRC", "DDColor"), "basicsr")
    for name, path in [("basicsr", src), ("basicsr.archs", os.path.join(src, "archs")), ("basicsr.utils", os.path.join(src, "utils"))]:
        package = types.ModuleType(name)
        package.__path__ = [path]
        sys.modules[name] = package
    registry = types.ModuleType("basicsr.utils.registry")
    registry.ARCH_REGISTRY = types.SimpleNamespace(register=lambda *a, **k: (lambda cls: cls))
    sys.modules["basicsr.utils.registry"] = registry
    from basicsr.archs.ddcolor_arch import DDColor

    return DDColor


def load(path):
    DDColor = _network()

    net = DDColor(
        encoder_name="convnext-t",
        decoder_name="MultiScaleColorDecoder",
        input_size=[512, 512],
        num_output_channels=2,
        last_norm="Spectral",
        do_normalize=False,
        num_queries=100,
        num_scales=3,
        dec_layers=9,
    )
    state = torch.load(path, map_location="cpu", weights_only=True)
    net.load_state_dict(state, strict=True)
    net.eval()
    _constant_positions(net)
    _plain_attention(net)
    return net


class _Attention(torch.nn.Module):
    """nn.MultiheadAttention (sequence first, no masks, inference) with its sizes as plain
    numbers: PyTorch's own works them out from the input's shape, which Core ML's
    converter folds wrongly. Same weights, same result."""

    def __init__(self, mha):
        super().__init__()
        self.heads = mha.num_heads
        self.dim = mha.embed_dim // mha.num_heads
        self.in_proj_weight = mha.in_proj_weight
        self.in_proj_bias = mha.in_proj_bias
        self.out_proj = mha.out_proj

    def forward(self, query, key, value, attn_mask=None, key_padding_mask=None, **_):
        assert attn_mask is None and key_padding_mask is None, "masks are not used by DDColor"
        e, h, d = self.heads * self.dim, self.heads, self.dim
        w, b = self.in_proj_weight, self.in_proj_bias
        q = torch.nn.functional.linear(query, w[:e], b[:e])
        k = torch.nn.functional.linear(key, w[e : 2 * e], b[e : 2 * e])
        v = torch.nn.functional.linear(value, w[2 * e :], b[2 * e :])
        lq, lk = query.shape[0], key.shape[0]
        # (length, batch 1, e) -> (heads, length, d)
        q = q.reshape(lq, h, d).transpose(0, 1) * (d**-0.5)
        k = k.reshape(lk, h, d).transpose(0, 1)
        v = v.reshape(lk, h, d).transpose(0, 1)
        out = torch.softmax(q @ k.transpose(1, 2), dim=-1) @ v
        out = out.transpose(0, 1).reshape(lq, 1, e)
        return self.out_proj(out), None


def _plain_attention(net):
    for parent in list(net.modules()):
        for name, child in list(parent.named_children()):
            if isinstance(child, torch.nn.MultiheadAttention):
                setattr(parent, name, _Attention(child))


def _constant_positions(net):
    """The decoder's sine position encodings depend only on the size of what they encode,
    which the fixed 512 x 512 input fixes: each is computed once and kept as a constant
    (computed from the size inside the graph, it doesn't convert to Core ML)"""
    from basicsr.archs.ddcolor_arch_utils.position_encoding import PositionEmbeddingSine

    found = {}
    hooks = [m.register_forward_hook(lambda m, i, o: found.setdefault(m, []).append(o.detach().clone()))
             for m in net.modules() if isinstance(m, PositionEmbeddingSine)]
    with torch.no_grad():
        net(torch.zeros(1, 3, 512, 512))
    for h in hooks:
        h.remove()
    for module, outputs in found.items():
        queue = list(outputs)

        def forward(x, mask=None, _all=outputs, _queue=queue):
            # Called in the same order on every run: hand out the same encodings in turn
            if not _queue:
                _queue.extend(_all)
            return _queue.pop(0)

        module.forward = forward
