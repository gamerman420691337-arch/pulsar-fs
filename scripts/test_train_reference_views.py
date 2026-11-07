"""Authored native staging, separate export ABI, and lineage refusal gates."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import shutil
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('annotated_export', Path(__file__).with_name('train-reference-views.py'))
export = importlib.util.module_from_spec(spec)
spec.loader.exec_module(export)
training = export.training


class AnnotatedExportTests(unittest.TestCase):
    def test_annotated_intake_cannot_downgrade_legacy_export(self):
        for intake in [{'schema_version': 1, 'input_views_sha256': '0' * 64},
                       {'schema_version': 1, 'entries': [{'input_view_plan': {}}]}]:
            with self.assertRaisesRegex(ValueError, 'separately bound model'):
                training.train(None, None, None, intake, 1, 1)

    def test_legacy_recipe_cannot_enter_annotated_export(self):
        with self.assertRaisesRegex(ValueError, 'bound recipe'):
            training.train_annotated(None, None, None, {'schema_version': 1}, 1, 1, None)

    @unittest.skipUnless(os.environ.get('PULSAR_TRAINING_VIEW_PLANNER'), 'native export gate requires a built planner')
    def test_actual_authored_training_exports_separate_abi_and_bound_recipe(self):
        import numpy as np
        import onnxruntime as ort
        planner = Path(os.environ['PULSAR_TRAINING_VIEW_PLANNER'])
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            live = root / 'Live-Reference'
            live.mkdir()
            entries, annotations = [], []
            for index, name in enumerate(('a', 'b')):
                video = live / (name + '.mkv')
                subprocess.run(['/usr/bin/ffmpeg', '-v', 'error', '-f', 'lavfi', '-i',
                    'testsrc2=size=64x64:rate=8:duration=3', '-vf', 'hflip' if index else 'null',
                    '-threads', '1', '-c:v', 'ffv1', str(video)], check=True)
                (live / (name + '.json')).write_text(json.dumps({'actions': [
                    {'at': 0, 'pos': 50 * index}, {'at': 1500, 'pos': 25 + 50 * index},
                    {'at': 3000, 'pos': 50 + 50 * index}]}))
                entries.append({'id': name, 'group': name, 'video': name + '.mkv',
                                'reference': name + '.json', 'permitted_uses': ['training']})
                annotations.append({'id': name, 'source_sha256': training.hash_file(video), 'declaration': {
                    'schema_version': 1, 'source_dimensions': [64, 64], 'analysis_dimensions': [64, 64],
                    'video_view': {'layout': 'mono', 'eye': 'left', 'projection': {'kind': 'flat'}},
                    'target_region': [8, 8, 32, 32], 'target_id': 'authored-' + name}})
            manifest_path = root / 'manifest.json'
            manifest_path.write_text(json.dumps({'schema_version': 1, 'split': 'training',
                'authorization': 'authored regression only', 'input_root': str(live), 'entries': entries}))
            annotation_path = root / 'views.json'
            annotation_path.write_text(json.dumps({'schema_version': 1, 'entries': annotations}))
            staged = root / 'staged'
            staged.mkdir()
            training.stage(manifest_path, staged, 1, 1, 1, '/usr/bin/ffmpeg', '/usr/bin/ffprobe',
                           annotation_path, planner)
            output = root / 'trained'
            command = [sys.executable, str(Path(export.__file__)), '--staged-run', str(staged),
                '--planner', str(planner), '--output', str(output), '--heldout-group', 'b', '--epochs', '1']
            completed = subprocess.run(command, capture_output=True, timeout=60)
            self.assertEqual(completed.returncode, 0, completed.stderr.decode())
            receipt = json.loads((output / 'training.json').read_text())
            self.assertEqual(receipt['schema_version'], 2)
            self.assertEqual(receipt['abi']['input_name'], 'target_frame_pair')
            self.assertEqual(receipt['input_recipe'], training.VIEW_MODEL_CONTRACT)
            self.assertEqual(receipt['training_samples'], 7)
            self.assertLessEqual(receipt['maximum_onnx_absolute_error'], 1e-5)
            development = json.loads((output / 'holdout.json').read_text())
            self.assertEqual(development['optimizer_samples'], 7)
            self.assertEqual(development['heldout_samples'], 7)
            self.assertFalse(development['sterile_evaluation_performed'])
            options = ort.SessionOptions()
            options.intra_op_num_threads = options.inter_op_num_threads = 1
            session = ort.InferenceSession(str(output / 'model.onnx'), options, providers=['CPUExecutionProvider'])
            metadata = session.get_modelmeta().custom_metadata_map
            self.assertEqual(json.loads(metadata['pulsar.temporal_recipe']), training.VIEW_MODEL_CONTRACT)
            self.assertEqual(metadata['pulsar.training_intake_sha256'], training.hash_file(output / 'intake.json'))
            evidence = os.environ.get('PULSAR_ANNOTATED_EXPORT_EVIDENCE')
            if evidence:
                destination = Path(evidence)
                destination.mkdir(mode=0o700, parents=False)
                for name, source in [('training', output), ('staging', staged)]:
                    target = destination / name
                    target.mkdir(mode=0o700)
                    for path in source.iterdir():
                        shutil.copyfile(path, target / path.name)
                        (target / path.name).chmod(0o400)
                report = {'scope': 'authored export fixture only; no quality qualification',
                          'training_samples': receipt['training_samples'],
                          'heldout_samples': development['heldout_samples'],
                          'maximum_onnx_absolute_error': receipt['maximum_onnx_absolute_error'],
                          'maximum_heldout_onnx_absolute_error': development['maximum_onnx_absolute_error'],
                          'model': receipt['model'], 'input_recipe': receipt['input_recipe'],
                          'private_fixture_root': str(destination)}
                (destination / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
                (destination / 'summary.json').chmod(0o400)
            with self.assertRaises(Exception):
                session.run(None, {'frame_pair': np.zeros((1, 6, 64, 64), np.float32)})
            before = {p.name: training.hash_file(p) for p in output.iterdir()}
            retry = subprocess.run(command, capture_output=True, timeout=15)
            self.assertNotEqual(retry.returncode, 0)
            self.assertEqual(before, {p.name: training.hash_file(p) for p in output.iterdir()})
            # An altered saved recipe is refused by fresh native planning before fitting.
            intake = json.loads((staged / 'intake.json').read_text())
            intake['entries'][0]['input_view_plan']['spatial_filter'] = 'null'
            (staged / 'intake.json').write_text(json.dumps(intake))
            refused = root / 'refused'
            rejected = subprocess.run([*command[:command.index('--output') + 1], str(refused),
                '--heldout-group', 'b', '--epochs', '1'], capture_output=True, timeout=15)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn('saved native view plan differs', rejected.stderr.decode())
            self.assertFalse(refused.exists())


if __name__ == '__main__':
    unittest.main()
