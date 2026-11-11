"""Train a small outline endpoint tracker on generated geometry only."""

import argparse
import json
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw
from torch import nn
from torch.nn import functional as F


def synthetic(count, seed):
    rng = np.random.default_rng(seed)
    samples = np.empty((count, 2, 64, 64), np.uint8)
    targets = np.empty((count, 2), np.float32)
    for i in range(count):
        x, y = rng.uniform(20, 58), rng.uniform(7, 57)
        body = rng.uniform(22, 53)
        tilt = rng.uniform(-0.78, 0.78)
        c, s = np.cos(tilt), np.sin(tilt)
        knuckle = rng.uniform(3, 6)
        tail = rng.uniform(2, 8)
        shape = [
            (-body, -tail),
            (-9, -2.7),
            (-3, -knuckle),
            (0, -knuckle),
            (2, -knuckle * 0.64),
            (2.5, 0),
            (2, knuckle * 0.7),
            (0, knuckle),
            (-4, knuckle * 0.78),
            (-9, 3),
            (-body, tail),
        ]
        delta_x, delta_y = rng.uniform(-4, 4, size=2)
        background = int(rng.uniform(0, 118))
        foreground = min(255, background + int(rng.uniform(80, 225)))
        if rng.random() < 0.22:
            background, foreground = foreground, background
        line = rng.uniform(0.45, 1.7)
        for k, (dx, dy) in enumerate(((delta_x, delta_y), (0, 0))):
            canvas = Image.new("L", (128, 128), color=background)
            draw = ImageDraw.Draw(canvas)
            p = [
                (
                    round(2 * (x + dx + c * a - s * b)),
                    round(2 * (y + dy + s * a + c * b)),
                )
                for a, b in shape
            ]
            if rng.random() < 0.3:
                draw.polygon(p, fill=foreground)
            else:
                draw.line(p, fill=foreground, width=round(2 * line), joint="curve")
            samples[i, k] = np.asarray(
                canvas.resize(
                    (64, 64),
                    rng.choice([Image.Resampling.BILINEAR, Image.Resampling.NEAREST]),
                )
            )
        targets[i] = (x, y)
    return torch.from_numpy(samples), torch.from_numpy(targets)


class Tracker(nn.Module):
    def __init__(self):
        super().__init__()
        self.features = nn.Sequential(
            nn.Conv2d(2, 12, 5, stride=2, padding=2),
            nn.BatchNorm2d(12),
            nn.ReLU(),
            nn.Conv2d(12, 16, 5, padding=2),
            nn.BatchNorm2d(16),
            nn.ReLU(),
            nn.Conv2d(16, 16, 5, padding=2),
            nn.BatchNorm2d(16),
            nn.ReLU(),
            nn.Conv2d(16, 1, 1),
        )
        self.register_buffer(
            "y", torch.arange(32).repeat_interleave(32).float() * 2 / 63
        )

    def scores(self, pair):
        return self.features(pair).flatten(1)

    def forward(self, pair):
        monochrome = torch.cat(
            (pair[:, :3].mean(1, keepdim=True), pair[:, 3:].mean(1, keepdim=True)), 1
        )
        probability = self.scores(monochrome).softmax(1)
        return (1 - (probability * self.y).sum(1, keepdim=True)).clamp(0, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "output", type=Path, help="New directory for the trained tracker"
    )
    root = parser.parse_args().output
    root.mkdir()
    torch.manual_seed(751)
    torch.set_num_threads(4)
    x, loc = synthetic(6400, 751)
    vx, vloc = synthetic(900, 752)
    net = Tracker()
    optim = torch.optim.AdamW(net.parameters(), lr=0.003)
    yy, xx = torch.meshgrid(torch.arange(32) * 2, torch.arange(32) * 2, indexing="ij")
    dist = (loc[:, 0, None, None] - xx) ** 2 + (loc[:, 1, None, None] - yy) ** 2
    probs = (-dist.flatten(1) / 12).softmax(1)
    for epoch in range(9):
        net.train()
        for batch in torch.randperm(len(x)).split(128):
            optim.zero_grad()
            loss = F.cross_entropy(net.scores(x[batch].float() / 255), probs[batch])
            loss.backward()
            optim.step()
        net.eval()
        with torch.no_grad():
            pred = torch.cat(
                [
                    (net.scores(batch.float() / 255).softmax(1) * net.y).sum(1) * 63
                    for batch in vx.split(128)
                ]
            )
        mae = (pred - vloc[:, 1]).abs().mean().item()
        print(f"epoch={epoch + 1} generated_holdout_mae_pixels={mae:.3f}", flush=True)

    assert mae < 4.0
    torch.onnx.export(
        net,
        (torch.zeros(1, 6, 64, 64),),
        root / "trained-outline-tracker.onnx",
        input_names=["frame_pair"],
        output_names=["script_position"],
        opset_version=17,
        dynamo=False,
    )
    (root / "training-summary.json").write_text(
        json.dumps(
            {
                "training": "independently generated contour geometry",
                "training_seed": 751,
                "synthetic_holdout_seed": 752,
                "training_pairs": len(x),
                "synthetic_holdout_pairs": len(vx),
                "synthetic_holdout_mae_pixels": mae,
                "epochs": 9,
                "projection": "1 - normalized vertical image coordinate",
                "sterile_clip_or_reference_used_in_training": False,
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
