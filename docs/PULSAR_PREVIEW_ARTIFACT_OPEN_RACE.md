# Preview artifact open-race checkpoint

Unix preview decoding now refuses a final symlink or FIFO substituted after pathname inspection. This closes a reproduced client artifact-consumption defect; it does not resolve cold native preparation.

## Failure and repair

The decoder checked `symlink_metadata`, then used ordinary `File::open` later. A deterministic filesystem cut replaces the checked regular frame with a symlink to identical valid RGB bytes. The original decoder follows it and returns success, violating its regular-file/no-symlink policy. A raced FIFO could instead block the desktop RPC thread while waiting for a writer.

Unix opening now uses the existing rustix dependency with `RDONLY | CLOEXEC | NOFOLLOW | NONBLOCK`. The opened descriptor must be a regular file of the declared size before allocation/read. The full size and SHA-256 checks and checked raw-to-RGBA layout remain enforced. Production supplies a no-op to the private deterministic cut helper; there is no mutable global test hook or public protocol change.

The symlink regression is red before the fix and green afterward. The FIFO regression returns an error with no writer under the bounded test command. Existing valid-frame and corrupt-hash cases still pass. Direct client tests pass 135 with three explicitly ignored; the preview subset passes 11 with one ignored. These are cached-dependency client tests, not a Cargo workspace or GUI qualification gate.

The current decoder is also compiled into a standalone replay driver against the original authenticated CUDA preview response and two retained RGB artifacts from the preceding shared-image campaign. Only artifact paths are relocated; identities, dimensions, context and model/profile evidence are preserved. Independent Python conversion matches all 16,384 output bytes of each RGBA frame. This is actual native-data decoder replay, not fresh GPU execution or a window-presentation test.

Offline locked CUDA release build and seven-file runtime assembly pass; every runtime file is independently rehashed. Architecture/source guards pass 289/36 and diff checks pass. The source checkpoint has 417 records: only `crates/pulsar-clients/src/preview.rs` changes, with 416 unchanged relative to the shared-image handoff checkpoint. Source identity is checked after build and before retention. Compiler warnings are retained; no commit or push occurs.

The [evidence packet](evidence/roadmap-2026-10-09/preview-artifact-open-race/checkpoint.json) contains source/patch snapshots, red/green logs, native response replay commands and complete RGBA outputs, the rebuilt candidate and the separate cold-copy investigation. Owned diagnostic services are terminal, and build/test/copy scratch is removed after integrity and process-reference checks.

## Scope still open

Descriptor flags are implemented for Unix and tested here on Linux. Windows/macOS qualification remains open. Nonblocking opening of a FIFO does not impose a wall deadline on ordinary regular-file filesystem I/O. Cold 30-second preparation, sustained live performance, useful real-media model quality, actual Stash interoperability, Handy/firmware playback, offline/platform packaging and human acceptance remain open. Full roadmap status remains PARTIAL.
