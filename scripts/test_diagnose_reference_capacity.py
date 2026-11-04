"""Authored optimizer-only capacity controls and split/identity refusal tests."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import numpy as np

spec = importlib.util.spec_from_file_location('capacity', Path(__file__).with_name('diagnose-reference-capacity.py'))
capacity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capacity)


class CapacityTests(unittest.TestCase):
    def test_selection_balances_entries_without_development_or_label_access(self):
        ids = np.asarray(['a'] * 12 + ['b'] * 12 + ['c'] * 12)
        optimizer = ids != 'c'
        rows = capacity.select_rows(ids, optimizer, 9, 123)
        self.assertEqual(rows.tolist(), capacity.select_rows(ids, optimizer, 9, 123).tolist())
        self.assertEqual(len(set(rows)), 9)
        self.assertEqual(sorted(np.unique(ids[rows], return_counts=True)[1].tolist()), [4, 5])
        self.assertFalse((ids[rows] == 'c').any())
        for count in (True, 7, 25, 257):
            with self.assertRaises(ValueError):
                capacity.select_rows(ids, optimizer, count, 123)

    def test_exact_duplicate_conflicts_have_the_correct_empirical_mae_floor(self):
        frames = np.zeros((3, 6, 64, 64), np.uint8)
        frames[2] = 255
        result = capacity.duplicate_summary(frames, np.asarray([[0], [1], [0.5]], np.float32))
        self.assertEqual(result['duplicate_buckets'], 1)
        self.assertEqual(result['conflicting_rows'], 2)
        self.assertAlmostEqual(result['empirical_exact_input_mae_lower_bound'], 1 / 3)

    def test_shared_network_learns_authored_signal_with_fixed_seed(self):
        frames = np.repeat(np.asarray([0, 85, 170, 255], np.uint8), 3)[:, None, None, None] * np.ones((12, 6, 64, 64), np.uint8)
        labels = (0.1 + 0.8 * frames[:, 0, 0, 0].astype(np.float32) / 255).reshape(-1, 1)
        a = capacity.fit_capacity(frames, labels, 20, 123)
        b = capacity.fit_capacity(frames, labels, 20, 123)
        self.assertEqual(a, b)
        self.assertLess(a['measurements'][-1]['subset_mae'], a['subset_mean_baseline_mae'] * 0.7)
        self.assertEqual([x['epoch'] for x in a['measurements']], [10, 20])
        with self.assertRaises(ValueError):
            capacity.fit_capacity(frames, labels.astype(np.float64), 1, 123)

    def fixture(self, root):
        staged = root / 'staged'
        staged.mkdir()
        live = root / 'Live-Reference'
        live.mkdir()
        for name in ['a', 'b']:
            (live / name).write_bytes(name.encode())
            (live / (name + '.json')).write_text('{}')
        manifest = {'schema_version': 1, 'split': 'training', 'authorization': 'authored fixtures',
                    'input_root': str(live), 'entries': [{'id': n, 'group': n, 'video': n,
                    'reference': n + '.json', 'permitted_uses': ['training']} for n in ['a', 'b']]}
        raw = json.dumps(manifest).encode()
        (staged / 'manifest.json').write_bytes(raw)
        np.savez(staged / 'dataset.npz', frames=np.zeros((20, 6, 64, 64), np.uint8),
                 labels=np.linspace(0, 1, 20, dtype=np.float32).reshape(-1, 1),
                 source_pts=np.asarray([[i, i + 1] for i in range(10)] * 2, np.int64), groups=np.asarray(['a'] * 10 + ['b'] * 10))
        intake = {'schema_version': 1, 'split': 'training', 'manifest_sha256': hashlib.sha256(raw).hexdigest(),
                  'dataset_sha256': capacity.training.hash_file(staged / 'dataset.npz'), 'samples': 20,
                  'entries': [{'id': n, 'group': n, 'source': {'sha256': n * 64}, 'reference': {'sha256': n.upper() * 64},
                               'source_timebase': '1/30', 'clips': [{'training_samples': 10}]} for n in ['a', 'b']]}
        intake_raw = json.dumps(intake).encode()
        (staged / 'intake.json').write_bytes(intake_raw)
        split = {'dataset_sha256': intake['dataset_sha256'], 'parent_intake_sha256': hashlib.sha256(intake_raw).hexdigest(),
                 'sterile_inputs_used': False, 'heldout_groups': ['b'], 'training_groups': ['a'],
                 'training_samples': 10, 'heldout_samples': 10}
        (staged / 'split.json').write_text(json.dumps(split))
        return staged

    def test_actual_controls_exclude_development_and_preserve_existing_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            staged = self.fixture(root)
            output = root / 'capacity'
            before = capacity.training.hash_file(staged / 'dataset.npz')
            result = capacity.run(staged, staged, output, 8, 1, 123)
            self.assertEqual(result['selected_samples'], 8)
            self.assertEqual(result['excluded_development_samples'], 10)
            self.assertFalse(result['development_fitted_or_evaluated'])
            self.assertFalse(result['model_saved_or_exported'])
            protocol = json.loads((output / 'protocol.json').read_text())
            self.assertTrue(all(i < 10 for i in protocol['selected_rows']))
            self.assertEqual(sorted(protocol['permuted_label_order']), list(range(8)))
            saved = (output / 'result.json').read_bytes()
            with self.assertRaises(FileExistsError):
                capacity.run(staged, staged, output, 8, 1, 123)
            self.assertEqual((output / 'result.json').read_bytes(), saved)
            self.assertFalse((output / 'failure.json').exists())
            self.assertEqual(capacity.training.hash_file(staged / 'dataset.npz'), before)

    def test_changed_dataset_or_split_refuses_before_any_fitting(self):
        for mutation in ['dataset', 'split']:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                staged = self.fixture(root)
                if mutation == 'dataset':
                    with (staged / 'dataset.npz').open('ab') as stream:
                        stream.write(b'changed')
                else:
                    split = json.loads((staged / 'split.json').read_text())
                    split['training_groups'] = ['b']
                    (staged / 'split.json').write_text(json.dumps(split))
                with mock.patch.object(capacity, 'fit_capacity') as fit:
                    with self.assertRaises(ValueError):
                        capacity.run(staged, staged, root / 'refused', 8, 1, 123)
                    fit.assert_not_called()
                self.assertTrue((root / 'refused/failure.json').is_file())


if __name__ == '__main__':
    unittest.main()
