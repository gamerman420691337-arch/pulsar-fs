#!/usr/bin/env python3
"""Stage explicitly annotated training views; never fit or activate a model."""
import argparse
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location('view_training', Path(__file__).with_name('train-reference-position.py'))
training = importlib.util.module_from_spec(spec)
spec.loader.exec_module(training)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--input-views', type=Path, required=True)
    parser.add_argument('--planner', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--windows', type=int, default=4)
    parser.add_argument('--duration', type=int, default=12)
    parser.add_argument('--stride', type=int, default=4)
    args = parser.parse_args()
    training.require(1 <= args.windows <= 16 and 1 <= args.duration <= 30 and 1 <= args.stride <= 60,
                     'staging parameter admission')
    training.validate_manifest(training.read_json(args.manifest)[0])
    training.read_json(args.input_views)
    args.output.mkdir(mode=0o700, parents=False)
    try:
        training.stage(args.manifest, args.output, args.windows, args.duration, args.stride,
                       '/usr/bin/ffmpeg', '/usr/bin/ffprobe', args.input_views, args.planner)
    except BaseException as error:
        training.write_json(args.output / 'failure.json', {
            'state': 'failed', 'error_type': type(error).__name__, 'message': str(error)[:2048],
            'qualification': 'unqualified',
        })
        raise


if __name__ == '__main__':
    main()
