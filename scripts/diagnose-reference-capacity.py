#!/usr/bin/env python3
"""Fixed optimizer-only capacity controls; no quality gate or model activation."""
import argparse
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

spec = importlib.util.spec_from_file_location('capacity_holdout', Path(__file__).with_name('train-reference-holdout.py'))
holdout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(holdout)
training = holdout.training
require = training.require


def select_rows(ids, optimizer_mask, count, seed):
    """Choose a fixed balanced subset by entry without inspecting any labels."""
    import numpy as np
    require(type(count) is int and 8 <= count <= 256, 'capacity sample bound')
    entries = sorted(set(ids[optimizer_mask]))
    require(entries and len(entries) <= count <= int(optimizer_mask.sum()), 'capacity subset size')
    generator = np.random.default_rng(seed)
    pools = {entry: generator.permutation(np.flatnonzero(optimizer_mask & (ids == entry))).tolist() for entry in entries}
    selected = []
    while len(selected) < count:
        for entry in entries:
            if pools[entry] and len(selected) < count:
                selected.append(pools[entry].pop())
    return np.asarray(sorted(selected), dtype=np.int64)


def duplicate_summary(frames, labels):
    """Exact input identities and their irreducible empirical constant-output MAE."""
    import numpy as np
    buckets = {}
    for index, frame in enumerate(frames):
        key = hashlib.sha256(frame.tobytes()).hexdigest()
        buckets.setdefault(key, []).append(index)
    duplicate = [rows for rows in buckets.values() if len(rows) > 1]
    for rows in duplicate:
        require(all(np.array_equal(frames[rows[0]], frames[i]) for i in rows), 'input digest collision')
    conflicts = [rows for rows in duplicate if np.ptp(labels[rows, 0]) > 0]
    error = sum(float(np.abs(labels[rows, 0].astype(np.float64) - np.median(labels[rows, 0].astype(np.float64))).sum()) for rows in conflicts)
    return {'samples': len(frames), 'distinct_inputs': len(buckets), 'duplicate_buckets': len(duplicate),
            'duplicate_rows': sum(map(len, duplicate)), 'conflicting_buckets': len(conflicts),
            'conflicting_rows': sum(map(len, conflicts)), 'empirical_exact_input_mae_lower_bound': error / len(frames)}


def fit_capacity(frames, labels, epochs, seed):
    """Fit the production candidate architecture on this subset only; save no model."""
    import numpy as np
    import torch
    require(type(epochs) is int and 1 <= epochs <= 100, 'capacity epoch bound')
    require(2 <= len(frames) <= 256 and frames.shape == (len(frames), 6, 64, 64) and frames.dtype == np.uint8, 'capacity frame contract')
    require(labels.shape == (len(frames), 1) and labels.dtype == np.float32 and np.isfinite(labels).all() and ((labels >= 0) & (labels <= 1)).all(), 'capacity label contract')
    torch.set_num_threads(4)
    torch.manual_seed(seed)
    torch.use_deterministic_algorithms(True)
    network = training.position_network(torch)
    inputs = torch.from_numpy(frames)
    targets = torch.from_numpy(labels)
    optimizer = torch.optim.Adam(network.parameters(), lr=0.001)
    generator = torch.Generator().manual_seed(seed)
    measurements = []
    for epoch in range(epochs):
        network.train()
        total = 0.0
        for indices in torch.randperm(len(inputs), generator=generator).split(32):
            prediction = network(inputs[indices].float() / 255)
            loss = torch.nn.functional.mse_loss(prediction, targets[indices])
            require(bool(torch.isfinite(loss)), 'nonfinite capacity loss')
            optimizer.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(network.parameters(), 1.0)
            optimizer.step()
            total += float(loss.detach()) * len(indices)
        if (epoch + 1) % 10 == 0 or epoch + 1 == epochs:
            network.eval()
            with torch.inference_mode():
                predictions = torch.cat([network(batch.float() / 255) for batch in inputs.split(32)]).numpy()
            require(np.isfinite(predictions).all(), 'nonfinite capacity predictions')
            measurements.append({'epoch': epoch + 1, 'optimizer_mse': total / len(inputs),
                                 'subset_mae': float(np.abs(predictions - labels).mean())})
    return {'torch_version': torch.__version__, 'device': 'cpu', 'threads': 4,
            'parameters': sum(p.numel() for p in network.parameters()), 'samples': len(inputs),
            'subset_mean_baseline_mae': float(np.abs(labels - labels.mean()).mean()),
            'subset_median_baseline_mae': float(np.abs(labels - np.median(labels)).mean()),
            'measurements': measurements, 'scope': 'resubstitution capacity only; no generalization measurement'}


def run(staged, dataset_run, output, count, epochs, seed):
    import numpy as np
    require(type(seed) is int and 0 <= seed < 2**31 and type(epochs) is int and 1 <= epochs <= 100, 'capacity seed/epoch admission')
    # No training is allowed until the whole admission and immutable selection
    # receipt have completed. Existing output directories are never reused.
    manifest, manifest_raw = training.read_json(staged / 'manifest.json')
    intake, intake_raw = training.read_json(dataset_run / 'intake.json')
    split, split_raw = training.read_json(dataset_run / 'split.json')
    training.validate_manifest(manifest)
    training.validate_reference_offsets(manifest, intake)
    require(intake['schema_version'] == 1 and intake['split'] == 'training' and 'input_views_sha256' not in intake and
            all('input_view_plan' not in entry for entry in intake['entries']), 'whole-frame training intake required')
    require(hashlib.sha256(manifest_raw).hexdigest() == intake['manifest_sha256'] and
            split['parent_intake_sha256'] == hashlib.sha256(intake_raw).hexdigest() and
            split['dataset_sha256'] == intake['dataset_sha256'] and split['sterile_inputs_used'] is False, 'capacity input identity differs')
    require({(e['id'], e['group']) for e in manifest['entries']} == {(e['id'], e['group']) for e in intake['entries']}, 'capacity entry declarations differ')
    output.mkdir(mode=0o700, parents=False)
    try:
        for name, source, raw in [('manifest.json', staged / 'manifest.json', manifest_raw),
                                  ('intake.json', dataset_run / 'intake.json', intake_raw),
                                  ('split.json', dataset_run / 'split.json', split_raw)]:
            copied = training.snapshot(source, output / name, training.JSON_LIMIT, training.JSON_LIMIT)
            require(copied['sha256'] == hashlib.sha256(raw).hexdigest(), 'capacity receipt changed during snapshot')
        copied = training.snapshot(dataset_run / 'dataset.npz', output / 'dataset.npz', 512 * 1024**2, 512 * 1024**2)
        require(copied['sha256'] == intake['dataset_sha256'], 'capacity dataset identity differs')
        frames, labels, pts, ids = holdout.load_dataset(output / 'dataset.npz')
        require(len(frames) == intake['samples'], 'capacity intake count differs')
        development, groups = holdout.partition(intake, ids, split['heldout_groups'])
        require(set(groups[~development]) == set(split['training_groups']) and
                int(development.sum()) == split['heldout_samples'] and int((~development).sum()) == split['training_samples'], 'capacity split differs')
        for entry in intake['entries']:
            base = Fraction(entry['source_timebase'])
            require(base > 0 and all(0 < (int(b) - int(a)) * base <= Fraction(1, 2) for a, b in pts[ids == entry['id']]), 'capacity pair clock differs')
        selected = select_rows(ids, ~development, count, seed)
        require(not development[selected].any(), 'development sample selected for capacity fitting')
        source = Path(__file__)
        sources = {p.name: training.hash_file(p) for p in [source, Path(training.__file__), Path(holdout.__file__)]}
        shuffled = np.random.default_rng(seed + 1).permutation(len(selected))
        protocol = {'schema_version': 1, 'task': 'optimizer-only capacity diagnostic', 'qualification': 'unqualified',
                    'dataset_sha256': copied['sha256'], 'parent_intake_sha256': hashlib.sha256(intake_raw).hexdigest(),
                    'split_sha256': hashlib.sha256(split_raw).hexdigest(), 'source_identities': sources,
                    'seed': seed, 'epochs': epochs, 'selected_rows': selected.tolist(), 'permuted_label_order': shuffled.tolist(),
                    'selection': 'round-robin entry-balanced, seeded row choice before fitting; labels not consulted',
                    'controls': ['original_labels', 'permuted_labels'], 'model_saved_or_exported': False,
                    'development_fitted_or_evaluated': False, 'sterile_inputs_used': False,
                    'stop_rule': 'fixed epochs; no checkpoint selection or promotion; capacity metrics are resubstitution only'}
        training.write_json(output / 'protocol.json', protocol)
        selected_frames, selected_labels = frames[selected].copy(), labels[selected].copy()
        results = {}
        for name, targets in [('original_labels', selected_labels), ('permuted_labels', selected_labels[shuffled].copy())]:
            print(json.dumps({'phase': 'capacity_control', 'control': name, 'samples': len(selected), 'epochs': epochs}), flush=True)
            results[name] = fit_capacity(selected_frames, targets, epochs, seed)
        require(training.hash_file(output / 'dataset.npz') == copied['sha256'] and
                all(training.hash_file(Path(__file__).with_name(name)) == digest for name, digest in sources.items()), 'capacity inputs/source changed during fitting')
        result = {'schema_version': 1, 'status': 'MEASURED', 'qualification': 'unqualified',
                  'scope': 'optimizer-only memorization and exact-input diagnostic; not model quality or acceptance',
                  'protocol_sha256': training.hash_file(output / 'protocol.json'), 'dataset_sha256': copied['sha256'],
                  'optimizer_samples_available': int((~development).sum()), 'excluded_development_samples': int(development.sum()),
                  'selected_samples': len(selected), 'selected_entries': len(set(ids[selected])),
                  'exact_inputs': duplicate_summary(selected_frames, selected_labels), 'controls': results,
                  'development_tensors_loaded': True, 'development_fitted_or_evaluated': False,
                  'sterile_inputs_used': False, 'model_saved_or_exported': False, 'model_activated': False,
                  'limitations': ['Memorization does not establish visual/script alignment, target observability or generalization.',
                                  'These live groups are consumed development data; no fresh independent test is created.',
                                  'No held-out, stroke, platform, Handy, human or release qualification.']}
        training.write_json(output / 'result.json', result)
        return result
    except BaseException as error:
        training.write_json(output / 'failure.json', {'state': 'failed', 'error_type': type(error).__name__,
                                                     'message': str(error)[:2048], 'qualification': 'unqualified'})
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-run', type=Path, required=True)
    parser.add_argument('--dataset-run', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=128)
    parser.add_argument('--epochs', type=int, default=100)
    parser.add_argument('--seed', type=int, default=20261008)
    args = parser.parse_args()
    run(args.staged_run, args.dataset_run, args.output, args.samples, args.epochs, args.seed)


if __name__ == '__main__':
    main()
