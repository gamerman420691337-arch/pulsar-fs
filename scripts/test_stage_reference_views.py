"""Native annotation planning/staging using authored media only."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('views', Path(__file__).with_name('stage-reference-views.py'))
views = importlib.util.module_from_spec(spec)
spec.loader.exec_module(views)
training = views.training


class AnnotatedViewTests(unittest.TestCase):
    def test_legacy_model_export_refuses_new_recipe_before_loading_ml_runtime(self):
        with self.assertRaisesRegex(ValueError, 'separately bound model'):
            training.train(None, None, None, {'schema_version': 2}, 1, 1)

    @unittest.skipUnless(os.environ.get('PULSAR_TRAINING_VIEW_PLANNER'), 'native gate requires a built Pulsar planner')
    def test_native_full_and_half_equirectangular_views_decode_the_declared_eye(self):
        import numpy as np
        planner = Path(os.environ['PULSAR_TRAINING_VIEW_PLANNER'])
        for kind, source_width in [('equirectangular360', 256), ('half_equirectangular180', 128)]:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                live = root / 'Live-Reference'
                live.mkdir()
                frame = np.zeros((64, source_width, 3), dtype=np.uint8)
                frame[:, :source_width // 2] = [200, 10, 30]
                frame[:, source_width // 2:] = [10, 200, 30]
                raw = root / 'authored.rgb'
                raw.write_bytes(frame.tobytes() * 24)
                video = live / 'source.mkv'
                subprocess.run(['/usr/bin/ffmpeg', '-v', 'error', '-f', 'rawvideo', '-pixel_format',
                    'rgb24', '-video_size', f'{source_width}x64', '-framerate', '8', '-i', str(raw),
                    '-threads', '1', '-c:v', 'ffv1', str(video)], check=True)
                (live / 'script.json').write_text(json.dumps({'actions': [{'at': 0, 'pos': 0},
                    {'at': 1500, 'pos': 50}, {'at': 3000, 'pos': 100}]}))
                manifest = {'schema_version': 1, 'split': 'training', 'authorization': 'authored fixture',
                    'input_root': str(live), 'entries': [{'id': 'one', 'group': 'one', 'video': 'source.mkv',
                    'reference': 'script.json', 'permitted_uses': ['training']}]}
                manifest_path = root / 'manifest.json'
                manifest_path.write_text(json.dumps(manifest))
                declaration = {'schema_version': 1, 'source_dimensions': [source_width, 64],
                    'analysis_dimensions': [96, 64], 'target_region': [8, 4, 32, 24], 'target_id': 'authored-target',
                    'video_view': {'layout': 'side_by_side_left_right', 'eye': 'right', 'projection': {
                    'kind': kind, 'viewport': {'yaw_degrees': 0.0, 'pitch_degrees': 0.0, 'roll_degrees': 0.0,
                    'horizontal_fov_degrees': 60.0, 'vertical_fov_degrees': 60.0}}}}
                annotation_path = root / 'views.json'
                annotation_path.write_text(json.dumps({'schema_version': 1, 'entries': [{'id': 'one',
                    'source_sha256': training.hash_file(video), 'declaration': declaration}]}))
                staged = root / 'staged'
                staged.mkdir()
                frames, _, intake = training.stage(manifest_path, staged, 1, 1, 1,
                    '/usr/bin/ffmpeg', '/usr/bin/ffprobe', annotation_path, planner)
                self.assertIn('v360=input=', intake['entries'][0]['input_view_plan']['spatial_filter'])
                self.assertEqual(frames.shape, (7, 6, 64, 64))
                for channel, value in enumerate([10, 200, 30, 10, 200, 30]):
                    self.assertTrue((frames[:, channel] == value).all())

    @unittest.skipUnless(os.environ.get('PULSAR_TRAINING_VIEW_PLANNER'), 'native gate requires a built Pulsar planner')
    def test_native_staging_selects_declared_eye_and_region_and_preserves_pts(self):
        import numpy as np
        planner = os.environ.get('PULSAR_TRAINING_VIEW_PLANNER')
        self.assertTrue(planner, 'set PULSAR_TRAINING_VIEW_PLANNER to the built Pulsar binary')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            live = root / 'Live-Reference'
            live.mkdir()
            # Exact lossless RGB: distinguish eyes and a smaller target region.
            frame = np.zeros((64, 192, 3), dtype=np.uint8)
            frame[:, :96] = [200, 10, 30]
            frame[:, 96:] = [10, 200, 30]
            frame[4:28, 104:136] = [10, 30, 200]
            raw = root / 'authored.rgb'
            raw.write_bytes(frame.tobytes() * 24)
            video = live / 'source.mkv'
            subprocess.run(['/usr/bin/ffmpeg', '-v', 'error', '-f', 'rawvideo', '-pixel_format',
                            'rgb24', '-video_size', '192x64', '-framerate', '8', '-i', str(raw),
                            '-threads', '1', '-c:v', 'ffv1', str(video)], check=True)
            (live / 'script.json').write_text(json.dumps({'actions': [{'at': 0, 'pos': 0},
                        {'at': 1500, 'pos': 50}, {'at': 3000, 'pos': 100}]}))
            manifest = {'schema_version': 1, 'split': 'training', 'authorization': 'authored fixture',
                        'input_root': str(live), 'entries': [{'id': 'one', 'group': 'one',
                        'video': 'source.mkv', 'reference': 'script.json', 'permitted_uses': ['training']}]}
            manifest_path = root / 'manifest.json'
            manifest_path.write_text(json.dumps(manifest))
            request = {'schema_version': 1, 'source_dimensions': [192, 64], 'analysis_dimensions': [96, 64],
                       'video_view': {'layout': 'side_by_side_left_right', 'eye': 'right',
                                      'projection': {'kind': 'flat'}},
                       'target_region': [8, 4, 32, 24], 'target_id': 'authored-target'}
            annotations = {'schema_version': 1, 'entries': [{'id': 'one',
                'source_sha256': hashlib.sha256(video.read_bytes()).hexdigest(), 'declaration': request}]}
            annotation_path = root / 'views.json'
            annotation_path.write_text(json.dumps(annotations))
            annotated = root / 'annotated'
            environment = dict(os.environ, PULSAR_STATE_DIR=str(root / 'unused-engine-state'))
            completed = subprocess.run([os.sys.executable, str(Path(views.__file__)),
                '--manifest', str(manifest_path), '--input-views', str(annotation_path),
                '--planner', planner, '--output', str(annotated), '--windows', '1',
                '--duration', '1', '--stride', '1'], check=True, capture_output=True, env=environment)
            self.assertIn(b'staged', completed.stdout)
            self.assertFalse((root / 'unused-engine-state').exists())
            intake = json.loads((annotated / 'intake.json').read_text())
            self.assertEqual(intake['schema_version'], 2)
            self.assertEqual(intake['input_views_sha256'], hashlib.sha256(annotation_path.read_bytes()).hexdigest())
            self.assertEqual(intake['view_planner']['sha256'], training.hash_file(planner))
            self.assertEqual(intake['entries'][0]['input_view_plan']['declaration'], request)
            self.assertEqual((annotated / 'input-views.json').read_bytes(), annotation_path.read_bytes())
            plain = root / 'plain'
            plain.mkdir()
            training.stage(manifest_path, plain, 1, 1, 1, '/usr/bin/ffmpeg', '/usr/bin/ffprobe')
            with np.load(annotated / 'dataset.npz', allow_pickle=False) as crop, np.load(plain / 'dataset.npz', allow_pickle=False) as full:
                self.assertTrue(np.array_equal(crop['source_pts'], full['source_pts']))
                self.assertTrue(np.array_equal(crop['labels'], full['labels']))
                self.assertFalse(np.array_equal(crop['frames'], full['frames']))
                for channel, value in enumerate([10, 30, 200, 10, 30, 200]):
                    self.assertTrue((crop['frames'][:, channel] == value).all())
            # Explicit input identity prevents annotations migrating to new media.
            annotations['entries'][0]['source_sha256'] = '0' * 64
            annotation_path.write_text(json.dumps(annotations))
            refused = root / 'refused'
            refused.mkdir()
            with self.assertRaisesRegex(ValueError, 'source identity'):
                training.stage(manifest_path, refused, 1, 1, 1, '/usr/bin/ffmpeg', '/usr/bin/ffprobe', annotation_path, Path(planner))
            # Existing output cannot receive failure bookkeeping or replacement.
            before = {p.name: training.hash_file(p) for p in annotated.iterdir()}
            retry = subprocess.run([os.sys.executable, str(Path(views.__file__)),
                '--manifest', str(manifest_path), '--input-views', str(annotation_path),
                '--planner', planner, '--output', str(annotated)], capture_output=True)
            self.assertNotEqual(retry.returncode, 0)
            self.assertEqual(before, {p.name: training.hash_file(p) for p in annotated.iterdir()})


if __name__ == '__main__':
    unittest.main()
