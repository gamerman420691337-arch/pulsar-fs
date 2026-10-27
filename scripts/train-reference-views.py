#!/usr/bin/env python3
"""Train/export a separately named annotated-view model; never activate it."""
import argparse
import hashlib
from fractions import Fraction
import importlib.util
import json
from pathlib import Path
import sys

spec = importlib.util.spec_from_file_location('annotated_holdout', Path(__file__).with_name('train-reference-holdout.py'))
holdout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(holdout)
training = holdout.training
require = training.require


def validate_views(intake, manifest, annotations, planner):
    """Replay native numeric planning before fitting; never execute saved filters."""
    require(intake['schema_version'] == 2 and intake['split'] == manifest['split'] == 'training' and
            intake['preprocessing']['recipe'] == training.VIEW_MODEL_CONTRACT['recipe'],
            'annotated training recipe required')
    require(set(annotations) == {'schema_version', 'entries'} and
            type(annotations['schema_version']) is int and annotations['schema_version'] == 1,
            'annotation schema differs')
    entries = intake['entries']
    declared = annotations['entries']
    require(isinstance(declared, list) and 1 <= len(declared) <= 64 and
            len(declared) == len(entries), 'annotation count differs')
    by_id = {}
    for value in declared:
        require(set(value) == {'id', 'source_sha256', 'declaration'} and value['id'] not in by_id,
                'annotation fields/identity differ')
        by_id[value['id']] = value
    require(set(by_id) == {entry['id'] for entry in entries} and
            {(entry['id'], entry['group']) for entry in entries} ==
            {(entry['id'], entry['group']) for entry in manifest['entries']}, 'annotation manifest identities differ')
    planner = Path(planner).resolve(strict=True)
    require(planner.is_file() and planner.stat().st_size == intake['view_planner']['byte_len'] and
            training.hash_file(planner) == intake['view_planner']['sha256'], 'native planner identity differs')
    for entry in entries:
        annotation = by_id[entry['id']]
        require(annotation['source_sha256'] == entry['source']['sha256'] and
                annotation['declaration']['source_dimensions'] == entry['source_dimensions'],
                'annotation source identity/dimensions differ')
        declaration = json.dumps(annotation['declaration'], separators=(',', ':'), allow_nan=False)
        require(len(declaration.encode()) <= 4096, 'annotation byte admission')
        raw, _ = training.bounded_process([str(planner), 'training-input-plan', declaration], 65536, 65536, 30)
        require(json.loads(raw) == entry['input_view_plan'], 'saved native view plan differs')
    require(training.hash_file(planner) == intake['view_planner']['sha256'], 'planner changed during validation')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-run', type=Path, required=True)
    parser.add_argument('--planner', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--heldout-group', action='append', required=True)
    parser.add_argument('--epochs', type=int, default=10)
    parser.add_argument('--seed', type=int, default=20261008)
    args = parser.parse_args()
    require(1 <= args.epochs <= 100, 'epoch admission')
    intake, intake_raw = training.read_json(args.staged_run / 'intake.json')
    manifest, manifest_raw = training.read_json(args.staged_run / 'manifest.json')
    annotations, annotation_raw = training.read_json(args.staged_run / 'input-views.json')
    require(hashlib.sha256(manifest_raw).hexdigest() == intake['manifest_sha256'] and
            hashlib.sha256(annotation_raw).hexdigest() == intake['input_views_sha256'],
            'manifest/annotation identity differs')
    training.validate_manifest(manifest)
    training.validate_reference_offsets(manifest, intake)
    validate_views(intake, manifest, annotations, args.planner)
    args.output.mkdir(mode=0o700, parents=False)
    try:
        for name, raw in [('manifest.json', manifest_raw), ('intake.json', intake_raw), ('input-views.json', annotation_raw)]:
            source = args.staged_run / name
            copied = training.snapshot(source, args.output / name, training.JSON_LIMIT, training.JSON_LIMIT)
            require(copied['sha256'] == hashlib.sha256(raw).hexdigest(), 'receipt changed during snapshot')
        copied = training.snapshot(args.staged_run / 'dataset.npz', args.output / 'dataset.npz', 512 * 1024**2, 512 * 1024**2)
        require(copied['sha256'] == intake['dataset_sha256'], 'dataset identity differs')
        x, y, pts, ids = holdout.load_dataset(args.output / 'dataset.npz')
        require(len(x) == intake['samples'], 'sample count differs')
        mask, groups = holdout.partition(intake, ids, args.heldout_group)
        for entry in intake['entries']:
            base = Fraction(entry['source_timebase'])
            require(base > 0 and all(0 < (int(b) - int(a)) * base <= Fraction(1, 2)
                    for a, b in pts[ids == entry['id']]), 'source timebase/pair gap differs')
        training.write_json(args.output / 'split.json', {
            'schema_version': 1, 'scope': 'annotated live-reference development split; not independent acceptance',
            'dataset_sha256': copied['sha256'], 'parent_intake_sha256': hashlib.sha256(intake_raw).hexdigest(),
            'training_groups': sorted(set(groups[~mask])), 'heldout_groups': sorted(set(groups[mask])),
            'training_samples': int((~mask).sum()), 'heldout_samples': int(mask.sum()),
            'selection': 'explicit declared groups before fitting; fixed epochs; no development checkpoint selection',
            'family_identity': 'manifest declarations; independently unverified',
            'sterile_inputs_used': False, 'qualification': 'unqualified',
        })
        training.write_json(args.output / 'invocation.json', {
            'argv': sys.argv, 'python_version': sys.version, 'epochs': args.epochs, 'seed': args.seed,
            'trainer_sha256': training.hash_file(Path(training.__file__)),
            'driver_sha256': training.hash_file(Path(__file__)), 'planner': intake['view_planner'],
            'network_required': False, 'activation_performed': False,
        })
        training.write_json(args.output / 'model-contract.json', training.VIEW_MODEL_CONTRACT)
        training.train_annotated(x[~mask], y[~mask], args.output, intake, args.epochs, args.seed,
                                 (x[mask], y[mask], groups[mask]))
    except BaseException as error:
        training.write_json(args.output / 'failure.json', {
            'state': 'failed', 'error_type': type(error).__name__, 'message': str(error)[:2048],
            'qualification': 'unqualified',
        })
        raise


if __name__ == '__main__':
    main()
