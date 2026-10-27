import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from fractions import Fraction
import numpy as np

spec = importlib.util.spec_from_file_location('audit', Path(__file__).with_name('audit-reference-labels.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class LabelAuditTests(unittest.TestCase):
    def test_exact_interpolation_and_interior_turns_exclude_boundaries_and_plateaus(self):
        actions = [(0, 0), (100, 100), (200, 0)]
        times = [a[0] for a in actions]
        self.assertEqual(audit.interpolate(actions, times, Fraction(50)), Fraction(1, 2))
        self.assertIsNone(audit.interpolate(actions, times, Fraction(-1)))
        self.assertEqual(audit.pair_detail(actions, times, 50, 150), (Fraction(1, 2), 1, Fraction(1), Fraction(0)))
        self.assertEqual(audit.pair_detail(actions, times, 100, 200)[1], 0)
        self.assertIsNone(audit.pair_detail(actions, times, -1, 100))
        plateau = [(0, 0), (100, 100), (200, 100), (300, 0)]
        self.assertEqual(audit.pair_detail(plateau, [a[0] for a in plateau], 0, 300), (Fraction(1), 0, Fraction(2), Fraction(0)))

    def fixture(self, root, corrupt_label=False, corrupt_pts=False):
        staged, run = root/'staged', root/'run'
        staged.mkdir(); run.mkdir()
        def write(path, value):
            path.write_text(json.dumps(value))
            return hashlib.sha256(path.read_bytes()).hexdigest()
        frames = np.zeros((4, 6, 64, 64), dtype=np.uint8)
        labels = np.full((4, 1), 0.5, dtype=np.float32)
        pts = np.asarray([[2000, 2100], [2100, 2200]]*2, dtype=np.int64)
        if corrupt_label:
            labels[0, 0] = 0.75
        if corrupt_pts:
            pts[0, 0] = 1999
        np.savez(run/'dataset.npz', frames=frames, labels=labels, source_pts=pts, groups=np.asarray(['aa', 'aa', 'bb', 'bb']))
        entries, declarations = [], []
        for name, group in [('aa', 'optimizer'), ('bb', 'development')]:
            path = staged/(name+'.funscript')
            digest = write(path, {'actions': [{'at': t, 'pos': p} for t, p in [(0, 0), (100, 100), (200, 0), (300, 100)]]})
            entries.append({'id': name, 'group': group, 'reference_offset_ms': 50,
                            'reference': {'path': path.name, 'sha256': digest, 'byte_len': path.stat().st_size},
                            'source_timebase': '1/1000', 'source_origin': '2',
                            'clips': [{'decoded_pts': [2000, 2100, 2200], 'training_samples': 2, 'omitted_pairs': 0}]})
            declarations.append({'id': name, 'group': group, 'reference_offset_ms': 50, 'permitted_uses': ['training']})
        manifest_sha = write(staged/'manifest.json', {'schema_version': 2, 'split': 'training', 'entries': declarations})
        dataset_sha = hashlib.sha256((run/'dataset.npz').read_bytes()).hexdigest()
        intake_sha = write(run/'intake.json', {'split': 'training', 'samples': 4, 'manifest_sha256': manifest_sha,
                                             'dataset_sha256': dataset_sha, 'entries': entries})
        write(run/'split.json', {'dataset_sha256': dataset_sha, 'parent_intake_sha256': intake_sha,
                                'sterile_inputs_used': False, 'training_groups': ['optimizer'], 'heldout_groups': ['development']})
        return staged, run

    def test_nonzero_origin_and_offset_reconstruct_all_rows_and_hidden_excursions(self):
        with tempfile.TemporaryDirectory() as directory:
            staged, run = self.fixture(Path(directory))
            result = audit.audit(staged, run)
            self.assertEqual(result['status'], 'PASS')
            self.assertEqual(result['aggregate']['samples'], 4)
            self.assertEqual(result['aggregate']['label_float32_mismatches'], 0)
            self.assertEqual(result['aggregate']['pairs_with_hidden_script_excursion'], 4)
            self.assertEqual(result['aggregate']['maximum_hidden_script_excursion'], 0.5)
            self.assertEqual(result['optimizer']['samples'], 2)
            self.assertFalse(result['frames_loaded_or_decoded'])

    def test_label_corruption_is_reported_without_repair(self):
        with tempfile.TemporaryDirectory() as directory:
            staged, run = self.fixture(Path(directory), corrupt_label=True)
            before = (run/'dataset.npz').read_bytes()
            result = audit.audit(staged, run)
            self.assertEqual(result['status'], 'FAIL')
            self.assertEqual(result['aggregate']['label_float32_mismatches'], 1)
            self.assertEqual((run/'dataset.npz').read_bytes(), before)

    def test_changed_row_correspondence_or_input_digest_refuses(self):
        with tempfile.TemporaryDirectory() as directory:
            staged, run = self.fixture(Path(directory), corrupt_pts=True)
            with self.assertRaisesRegex(ValueError, 'row correspondence'):
                audit.audit(staged, run)
        with tempfile.TemporaryDirectory() as directory:
            staged, run = self.fixture(Path(directory))
            with (staged/'aa.funscript').open('a') as stream:
                stream.write(' ')
            with self.assertRaisesRegex(ValueError, 'snapshot identity'):
                audit.audit(staged, run)


if __name__ == '__main__':
    unittest.main()
