"""Saved desktop-view binding, native staging and no-overwrite refusal tests."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('assemble_views', Path(__file__).with_name('assemble-reference-views.py'))
views = importlib.util.module_from_spec(spec)
spec.loader.exec_module(views)


class SavedViewIntakeTests(unittest.TestCase):
    def test_strict_documents_refuse_duplicate_and_nonfinite_fields(self):
        for raw in [b'{"x":1,"x":2}', b'{"x":{"y":1,"y":2}}', b'{"x":NaN}', b'{"x":Infinity}']:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                views.strict_json(raw)

    def test_publisher_never_replaces_files_directories_or_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            file = root / 'existing-file'; file.write_bytes(b'original')
            empty = root / 'empty'; empty.mkdir()
            nonempty = root / 'nonempty'; nonempty.mkdir(); (nonempty / 'keep').write_bytes(b'original')
            link = root / 'link'; link.symlink_to(file)
            for path in [file, empty, nonempty, link]:
                with self.subTest(path=path), self.assertRaises(FileExistsError):
                    views.publish_new(path, {'input-views.json': b'new'})
            self.assertEqual(file.read_bytes(), b'original')
            self.assertEqual((nonempty / 'keep').read_bytes(), b'original')
            self.assertEqual(list(empty.iterdir()), [])
            self.assertTrue(link.is_symlink())
            self.assertEqual(list(root.glob('.pulsar-views-*')), [])
            output = root / 'new'; views.publish_new(output, {'input-views.json': b'new'})
            self.assertEqual((output / 'input-views.json').read_bytes(), b'new')
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
            self.assertEqual((output / 'input-views.json').stat().st_mode & 0o777, 0o600)

    def test_stage_name_collision_never_removes_unowned_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); stage = root / '.pulsar-views-fixed'; stage.mkdir()
            with mock.patch.object(views.uuid, 'uuid4', return_value=mock.Mock(hex='fixed')):
                with self.assertRaises(FileExistsError):
                    views.publish_new(root / 'new', {'input-views.json': b'new'})
            self.assertTrue(stage.is_dir())
            self.assertEqual(list(stage.iterdir()), [])

    def test_failed_publication_preserves_replaced_staging_file(self):
        import ctypes
        import errno
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            def fail_with_replacement(*_):
                stage = next(root.glob('.pulsar-views-*'))
                replacement = stage / 'replacement'
                replacement.write_bytes(b'other actor content')
                replacement.replace(stage / 'input-views.json')
                ctypes.set_errno(errno.EEXIST)
                return -1
            rename = mock.Mock(side_effect=fail_with_replacement)
            with mock.patch.object(views.ctypes, 'CDLL', return_value=mock.Mock(renameat2=rename)):
                with self.assertRaisesRegex(ValueError, 'staging content changed'):
                    views.publish_new(root / 'new', {'input-views.json': b'ours'})
            self.assertFalse((root / 'new').exists())
            stage = next(root.glob('.pulsar-views-*'))
            self.assertEqual((stage / 'input-views.json').read_bytes(), b'other actor content')

    @unittest.skipUnless(os.environ.get('PULSAR_TRAINING_VIEW_PLANNER'), 'native planner must be configured')
    def test_native_saved_view_assembles_and_stages_exact_selected_pixels_and_clock(self):
        import numpy as np
        planner = Path(os.environ['PULSAR_TRAINING_VIEW_PLANNER'])
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); live = root / 'Live-Reference'; live.mkdir()
            frame = np.zeros((64, 192, 3), dtype=np.uint8)
            frame[:, :96] = [200, 10, 30]; frame[:, 96:] = [10, 200, 30]
            frame[4:28, 104:136] = [10, 30, 200]
            raw = root / 'authored.rgb'; raw.write_bytes(frame.tobytes() * 24)
            video = live / 'source.mkv'
            subprocess.run(['/usr/bin/ffmpeg', '-nostdin', '-v', 'error', '-f', 'rawvideo', '-pixel_format', 'rgb24',
                '-video_size', '192x64', '-framerate', '8', '-i', str(raw), '-threads', '1', '-c:v', 'ffv1', str(video)],
                check=True, timeout=30)
            (live / 'script.json').write_text(json.dumps({'actions': [{'at': 0, 'pos': 0},
                {'at': 1500, 'pos': 50}, {'at': 3000, 'pos': 100}]}))
            manifest = {'schema_version': 2, 'split': 'training', 'authorization': 'authored test only',
                'input_root': str(live), 'entries': [{'id': 'one', 'group': 'one', 'video': 'source.mkv',
                'reference': 'script.json', 'reference_offset_ms': 125, 'permitted_uses': ['training']}]}
            manifest_path = root / 'manifest.json'; manifest_raw = json.dumps(manifest).encode(); manifest_path.write_bytes(manifest_raw)
            declaration = {'schema_version': 1, 'source_dimensions': [192, 64], 'analysis_dimensions': [96, 64],
                'video_view': {'layout': 'side_by_side_left_right', 'eye': 'right', 'projection': {'kind': 'flat'}},
                'target_region': [8, 4, 32, 24], 'target_id': 'authored-target'}
            saved = {'source_sha256': views.training.hash_file(video), 'declaration': declaration}
            sidecar = root / 'desktop-view.json'; sidecar_raw = json.dumps(saved).encode(); sidecar.write_bytes(sidecar_raw)
            output = root / 'assembled'
            receipt = views.assemble(manifest_path, {'one': sidecar}, planner, '/usr/bin/ffprobe', output)
            self.assertFalse(receipt['qualified'])
            self.assertEqual(receipt['entries'][0]['reference_offset_ms'], 125)
            self.assertEqual(receipt['entries'][0]['human_target_and_script_alignment'], 'unverified')
            self.assertEqual((output / 'manifest.json').read_bytes(), manifest_raw)
            self.assertEqual((output / 'one.view.json').read_bytes(), sidecar_raw)
            combined = json.loads((output / 'input-views.json').read_text())
            self.assertEqual(combined, {'schema_version': 1, 'entries': [{'id': 'one', **saved}]})
            stage = root / 'staged'; stage.mkdir()
            frames, labels, intake = views.training.stage(manifest_path, stage, 1, 1, 1,
                '/usr/bin/ffmpeg', '/usr/bin/ffprobe', output / 'input-views.json', planner)
            self.assertEqual(frames.shape, (7, 6, 64, 64))
            for channel, value in enumerate([10, 30, 200, 10, 30, 200]):
                self.assertTrue((frames[:, channel] == value).all())
            plain = root / 'plain'; plain.mkdir()
            _, plain_labels, plain_intake = views.training.stage(manifest_path, plain, 1, 1, 1,
                '/usr/bin/ffmpeg', '/usr/bin/ffprobe')
            self.assertTrue(np.array_equal(labels, plain_labels))
            self.assertEqual(intake['entries'][0]['clips'][0]['decoded_pts'], plain_intake['entries'][0]['clips'][0]['decoded_pts'])
            from fractions import Fraction
            clock = intake['entries'][0]
            expected_labels = np.asarray([float((p * Fraction(clock['source_timebase']) -
                Fraction(clock['source_origin']) + Fraction(1, 8)) / 3)
                for p in clock['clips'][0]['decoded_pts'][1:]], dtype=np.float32).reshape(-1, 1)
            self.assertTrue(np.array_equal(labels, expected_labels))
            print(json.dumps({'phase': 'saved_view_native_correspondence', 'qualified': False,
                'assembly_receipt': receipt, 'dataset_sha256': intake['dataset_sha256'],
                'decoded_pts': clock['clips'][0]['decoded_pts'],
                'source_timebase': clock['source_timebase'], 'source_origin': clock['source_origin'],
                'expected_fixture_labels': expected_labels.tolist(),
                'input_views_sha256': intake['input_views_sha256'],
                'scope': 'authored lossless stereo pixels and declared 125 ms fixture offset only'}), flush=True)
            cli_output = root / 'cli-output'
            command = [os.sys.executable, str(Path(views.__file__)), '--manifest', str(manifest_path),
                '--view', 'one=' + str(sidecar), '--planner', str(planner), '--ffprobe', '/usr/bin/ffprobe',
                '--output', str(cli_output)]
            result = subprocess.run(command, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)['entries'], 1)
            self.assertEqual((cli_output / 'input-views.json').read_bytes(), (output / 'input-views.json').read_bytes())
            duplicate = subprocess.run(command + ['--view', 'one=' + str(sidecar)],
                capture_output=True, text=True, timeout=60)
            self.assertNotEqual(duplicate.returncode, 0)
            self.assertIn('duplicate view assignment', duplicate.stderr)
            original_output = {p.name: p.read_bytes() for p in output.iterdir()}
            with self.assertRaises(FileExistsError):
                views.assemble(manifest_path, {'one': sidecar}, planner, '/usr/bin/ffprobe', output)
            self.assertEqual(original_output, {p.name: p.read_bytes() for p in output.iterdir()})
            bad = copy.deepcopy(saved); bad['source_sha256'] = '0' * 64
            invalid = root / 'invalid.json'; invalid.write_text(json.dumps(bad))
            for mapping, pattern in [({}, 'exact manifest IDs'), ({'one': invalid}, 'source identity')]:
                with self.subTest(pattern=pattern), self.assertRaisesRegex(ValueError, pattern):
                    views.assemble(manifest_path, mapping, planner, '/usr/bin/ffprobe', root / 'refused')
                self.assertFalse((root / 'refused').exists())
            bad = copy.deepcopy(saved); bad['declaration']['source_dimensions'] = [190, 64]
            invalid.write_text(json.dumps(bad))
            with self.assertRaisesRegex(ValueError, 'source dimensions'):
                views.assemble(manifest_path, {'one': invalid}, planner, '/usr/bin/ffprobe', root / 'refused')
            link = root / 'view-link'; link.symlink_to(sidecar)
            with self.assertRaises(OSError):
                views.assemble(manifest_path, {'one': link}, planner, '/usr/bin/ffprobe', root / 'refused')
            self.assertFalse((root / 'refused').exists())
            source_raw = video.read_bytes()
            bounded = views.training.bounded_process
            def mutate_source(command, *args, **kwargs):
                value = bounded(command, *args, **kwargs)
                if command[1] == 'training-input-plan':
                    with video.open('ab') as f:
                        f.write(b'changed')
                return value
            with mock.patch.object(views.training, 'bounded_process', side_effect=mutate_source):
                with self.assertRaisesRegex(ValueError, 'source changed'):
                    views.assemble(manifest_path, {'one': sidecar}, planner, '/usr/bin/ffprobe', root / 'refused')
            self.assertFalse((root / 'refused').exists())
            video.write_bytes(source_raw)
            def mutate_sidecar(command, *args, **kwargs):
                value = bounded(command, *args, **kwargs)
                if command[1] == 'training-input-plan':
                    sidecar.write_bytes(sidecar_raw + b' ')
                return value
            with mock.patch.object(views.training, 'bounded_process', side_effect=mutate_sidecar):
                with self.assertRaisesRegex(ValueError, 'saved view changed'):
                    views.assemble(manifest_path, {'one': sidecar}, planner, '/usr/bin/ffprobe', root / 'refused')
            self.assertFalse((root / 'refused').exists())
            self.assertEqual(list(root.glob('.pulsar-views-*')), [])


if __name__ == '__main__':
    unittest.main()
