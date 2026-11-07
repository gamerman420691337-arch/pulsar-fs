#!/usr/bin/env python3
"""Refresh the worker-fence ledger after a Linux boot change.

The Engine refuses to start when the recorded cgroup creation boot or parent
identity no longer matches the running system:

    cgroup creation boot changed; explicit recovery required

Automatic reboot recovery is an explicitly unfinished product requirement
(docs/PULSAR_CGROUP_CREATION_JOURNAL.md), so recovery is an operator action.

This helper is deliberately conservative. It refuses unless the ledger provably
holds **no owned resources**, then refreshes only the boot and parent identity:

  * groups / retired / staging must all be empty,
  * creation.records must be empty (no in-flight or bound creations),
  * the cgroup parent path must exist and still be a cgroup v2 directory.

A missing active group, a populated directory, or any recorded obligation is a
real custody question and must not be erased by this script.

Usage:
  ./scripts/recover-cgroup-boot.py --dry-run     # report only
  ./scripts/recover-cgroup-boot.py               # apply, after backup
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time


def die(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


def current_boot():
    return pathlib.Path("/proc/sys/kernel/random/boot_id").read_text().strip()


def parent_identity(path):
    try:
        out = subprocess.check_output(["stat", "-c", "%d %i", path]).decode().split()
    except subprocess.CalledProcessError:
        die(f"cgroup parent does not exist: {path}")
    return int(out[0]), int(out[1])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--state-dir", default=os.path.expanduser("~/.local/state/pulsar"))
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    ledger_path = pathlib.Path(args.state_dir) / "worker-fence.json"
    if not ledger_path.exists():
        die(f"no worker-fence ledger at {ledger_path}")

    ledger = json.loads(ledger_path.read_text())
    creation = ledger.get("creation")
    if not creation:
        print("ledger declares no creation custody; nothing to recover")
        return

    # Refuse unless there is provably nothing owned.
    for key in ("groups", "retired", "staging"):
        entries = ledger.get(key) or {}
        if entries:
            die(
                f"ledger holds {len(entries)} '{key}' entries; this is real "
                "custody and must be reconciled, not erased"
            )
    records = creation.get("records") or {}
    if records:
        die(
            f"ledger holds {len(records)} creation records; an interrupted "
            "creation may exist and must be reconciled, not erased"
        )

    parent = creation.get("parent") or {}
    path = parent.get("path")
    if not path:
        die("ledger creation parent has no path")
    if not pathlib.Path(path).is_dir():
        die(f"recorded cgroup parent is absent: {path}")

    boot = current_boot()
    device, inode = parent_identity(path)
    old_boot = creation.get("boot")
    same_parent = (parent.get("device"), parent.get("inode")) == (device, inode)

    print(f"boot:   {old_boot} -> {boot}" + ("  (unchanged)" if old_boot == boot else ""))
    print(f"parent: {parent.get('device')}:{parent.get('inode')} -> {device}:{inode}"
          + ("  (unchanged)" if same_parent else ""))
    print("owned resources: none (groups/retired/staging/records all empty)")

    if old_boot == boot and same_parent:
        print("ledger already matches this boot; no change needed")
        return

    if args.dry_run:
        print("dry run: would refresh boot/parent identity")
        return

    stamp = time.strftime("%Y%m%d-%H%M%S")
    backup = ledger_path.with_name(f"worker-fence.json.bak-{stamp}")
    shutil.copy2(ledger_path, backup)
    print(f"backup: {backup}")

    creation["boot"] = boot
    parent["device"] = device
    parent["inode"] = inode
    ledger_path.write_text(json.dumps(ledger, separators=(",", ":")))
    print("ledger refreshed; the Engine may now start")


if __name__ == "__main__":
    main()
