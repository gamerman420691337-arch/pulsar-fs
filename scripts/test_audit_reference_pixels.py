"""Authored training pixel correspondence and refusal regressions."""
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('pixels', Path(__file__).with_name('audit-reference-pixels.py'))
pixels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pixels)


class PixelAuditTests(unittest.TestCase):
    def pair_fixture(self):
        import numpy as np
        rgb = bytes([31, 63, 127]) * (64 * 64) + bytes([11, 23, 47]) * (64 * 64)
        image = np.frombuffer(rgb, dtype=np.uint8).reshape(2, 64, 64, 3)
        frames = np.concatenate([image[0], image[1]], axis=2).transpose(2, 0, 1)[None].copy()
        arrays = frames, np.asarray([[0.5]], dtype=np.float32), np.asarray([[0, 1]], dtype=np.int64), np.asarray(['one'])
        entry = {'id': 'one', 'source_timebase': '1/2', 'source_origin': '0',
                 '_actions': [(0, 0), (1000, 100)]}
        clip = {'decoded_pts': [0, 1], 'raw_rgb_sha256': hashlib.sha256(rgb).hexdigest(),
                'training_samples': 1, 'omitted_pairs': 0}
        return rgb, entry, clip, arrays

    def test_all_pixels_and_temporal_channel_order_are_checked(self):
        rgb, entry, clip, arrays = self.pair_fixture()
        self.assertEqual(pixels.check_clip(rgb, [0, 1], entry, clip, arrays, 0), (1, 1))
        arrays[0][0] = arrays[0][0, [3, 4, 5, 0, 1, 2]]
        with self.assertRaisesRegex(ValueError, 'temporal pixels'):
            pixels.check_clip(rgb, [0, 1], entry, clip, arrays, 0)

    def test_changed_pts_and_rgb_receipt_refuse(self):
        rgb, entry, clip, arrays = self.pair_fixture()
        with self.assertRaisesRegex(ValueError, 'clip PTS'):
            pixels.check_clip(rgb, [0, 2], entry, clip, arrays, 0)
        with self.assertRaisesRegex(ValueError, 'clip RGB'):
            pixels.check_clip(bytes([99]) + rgb[1:], [0, 1], entry, clip, arrays, 0)
        arrays[2][0, 1] = 2
        with self.assertRaisesRegex(ValueError, 'identity/PTS'):
            pixels.check_clip(rgb, [0, 1], entry, clip, arrays, 0)

    def test_framehash_correspondence_clock_and_count_refuse(self):
        rgb, _, _, _ = self.pair_fixture()
        lines = ['#tb 0: 1/2', '#hash: SHA256', '#dimensions 0: 64x64']
        for index in range(2):
            payload = rgb[index * pixels.FRAME_BYTES:(index + 1) * pixels.FRAME_BYTES]
            lines.append(f'0, {index}, {index}, 1, {len(payload)}, {hashlib.sha256(payload).hexdigest()}')
        receipt = ('\n'.join(lines) + '\n').encode()
        self.assertEqual(pixels.parse_frames(rgb, receipt, Fraction(1, 2)), [0, 1])
        for data, record, base in [(bytes([99]) + rgb[1:], receipt, Fraction(1, 2)),
                                   (rgb, receipt, Fraction(1, 3)),
                                   (rgb, ('\n'.join(lines[:-1]) + '\n').encode(), Fraction(1, 2))]:
            with self.assertRaises(ValueError):
                pixels.parse_frames(data, record, base)

    def test_snapshot_hash_and_symlink_refuse(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'source'
            source.write_bytes(b'original')
            record = {'path': 'source', 'byte_len': 8, 'sha256': hashlib.sha256(b'original').hexdigest()}
            self.assertEqual(pixels.snapshot_path(root, record, 100), source)
            source.write_bytes(b'modified')
            with self.assertRaisesRegex(ValueError, 'hash'):
                pixels.snapshot_path(root, record, 100)
            source.unlink()
            source.symlink_to(root / 'absent')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                pixels.snapshot_path(root, record, 100)

    def test_native_single_pass_decode_matches_authored_staging(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            live = root / 'Live-Reference'
            live.mkdir()
            video = live / 'source.mkv'
            subprocess.run(['/usr/bin/ffmpeg', '-v', 'error', '-f', 'lavfi', '-i',
                            'testsrc2=size=96x64:rate=8:duration=3', '-threads', '1',
                            '-c:v', 'ffv1', str(video)], check=True)
            (live / 'script.json').write_text(json.dumps({'actions': [{'at': 0, 'pos': 0},
                                                                    {'at': 1500, 'pos': 50},
                                                                    {'at': 3000, 'pos': 100}]}))
            manifest = {'schema_version': 1, 'split': 'training', 'authorization': 'authored only',
                        'input_root': str(live), 'entries': [{'id': 'one', 'group': 'one',
                        'video': 'source.mkv', 'reference': 'script.json', 'permitted_uses': ['training']}]}
            path = root / 'manifest.json'
            path.write_text(json.dumps(manifest))
            staged = root / 'staged'
            staged.mkdir()
            pixels.training.stage(path, staged, 1, 1, 1, '/usr/bin/ffmpeg', '/usr/bin/ffprobe')
            result = pixels.audit(staged, staged / 'dataset.npz', Path('/usr/bin/ffmpeg'), 1)
            self.assertEqual(result['status'], 'PASS')
            self.assertEqual(result['samples'], 7)
            self.assertEqual(len(result['clips']), 1)


if __name__ == '__main__':
    unittest.main()
