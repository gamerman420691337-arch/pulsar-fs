#!/usr/bin/env python3
"""Compile the pinned runtime's actual cleanup functions against musl.

The original nftw descriptor limit reports success without removing the owned
fixture. This regression checks filesystem effects, not just the return code.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("--cc", default="/usr/bin/musl-gcc")
    args = parser.parse_args()
    if not args.source.is_absolute() or args.source.stat().st_size > 256 * 1024:
        parser.error("a bounded absolute runtime C source is required")
    source = args.source.read_text()
    start = source.index("int rm_recursive_callback(")
    end = source.index("void build_mount_point(", start)
    functions = source[start:end]
    if functions.count("bool rm_recursive(") != 1 or "nftw(" not in functions:
        parser.error("unsupported upstream cleanup function layout")
    with tempfile.TemporaryDirectory(prefix="pulsar-cleanup-regression-") as root:
        root = Path(root)
        fixture = root / "fixture"
        fixture.mkdir(mode=0o700)
        (fixture / "nested").mkdir(mode=0o700)
        (fixture / "nested" / "payload").write_bytes(b"authored fixture")
        target = root / "external-preserved"
        target.write_bytes(b"must remain")
        (fixture / "external-link").symlink_to(target)
        program = root / "cleanup.c"
        program.write_text(
            "#define _XOPEN_SOURCE 700\n"
            "#include <ftw.h>\n#include <stdbool.h>\n#include <stdio.h>\n#include <string.h>\n"
            "#include <sys/stat.h>\n#include <errno.h>\n#include <unistd.h>\n"
            + functions
            + '\nint main(int argc, char **argv) {\n'
            + ' if (argc != 2) return 2;\n'
            + ' bool reported = rm_recursive(argv[1]);\n'
            + ' bool removed = access(argv[1], F_OK) != 0 && errno == ENOENT;\n'
            + ' printf("reported_success=%d directory_removed=%d\\n", reported, removed);\n'
            + ' return reported && removed ? 0 : 1;\n}\n'
        )
        executable = root / "cleanup"
        compiled = subprocess.run(
            [args.cc, "-static", "-Wall", "-Werror", str(program), "-o", str(executable)],
            capture_output=True, text=True, timeout=30, check=False,
        )
        if compiled.returncode:
            raise RuntimeError(compiled.stderr)
        observed = subprocess.run(
            [str(executable), str(fixture)], capture_output=True, text=True,
            timeout=10, check=False, env={"PATH": "/usr/bin:/bin"},
        )
        print(observed.stdout, end="")
        if observed.stderr:
            print(observed.stderr, end="")
        if target.read_bytes() != b"must remain":
            raise RuntimeError("cleanup followed the external fixture symlink")
        if observed.returncode or fixture.exists():
            raise RuntimeError("runtime reported cleanup without removing its owned fixture")
        print("external_symlink_target=preserved")


if __name__ == "__main__":
    main()
