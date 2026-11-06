# Supervised native dependency capture

`native_capture::capture` runs library copying and full hashing in a separate
Linux process. The engine reserves RAM, a CPU worker slot and expected output
storage before launch. A delegated cgroup bounds RAM/CPU/processes; bubblewrap
isolates the helper with read-only inputs and one writable, newly owned private
capture directory. This role never loads the captured libraries and does not
receive a project store or a device node.

The parent selects regular exact-length source descriptors without following
final symlinks. The child checks mounted filesystem/device/inode coordinates
against those selections, captures expected bytes and verifies its image. The
reply binds the attempt and complete serialized request, including budgets and
input coordinates. Parent adoption checks directory ownership, alias membership,
read-only content metadata and matching alias inodes without rehashing gigabytes
inside the authority process. Capture and staging directories explicitly use
mode `0700`; the temporary-directory library's defaults depend on the umask.

The parent uses nonblocking stdin/stdout/stderr, bounded replies/diagnostics and
one absolute deadline. Cancellation, malformed replies, child failures and
timeouts retire the child tree. RAM and its worker slot are released only after
observed process exit and an empty cgroup. Storage stays charged while the image
exists and is released when that image is closed.

If immediate retirement is unconfirmed, a background reaper keeps the process,
cgroup, staging directory and resource reservation. It releases them only after
retirement evidence. If reaper creation fails, custody is deliberately retained
rather than releasing capacity for a potentially live child. This is fail-closed
accounting, not a promise that the kernel can stop uninterruptible I/O. Parent
setup, metadata checks and cleanup still perform filesystem syscalls; the
supervised deadline is not an independently qualified bound for the entire API.

The [validation packet](evidence/roadmap-2026-10-01/supervised-native-capture/validation.json)
records exact tested source and binary subjects, full gates and native cases.
Authored inputs cover successful capture, wrong bytes, symlinks, FIFOs,
pre-cancellation and invalid budgets. The declared 74-library image separately
exercises full capture/readback, deadline and cancellation followed by observed
cleanup. These are individual measurements, not a latency distribution.

Production CUDA jobs remain disabled. Integrate the owned capture with fresh GPU
admission, durable-attempt schema barriers, mounted image/device verification,
the actual confined inference session and result/cancellation/restart fencing
before enabling them. Existing `CapturedNativeDependencies::verify` and the
owner-policy declaration builder still rehash synchronously when directly
called; their launch integration must not put that work back on the authority
control path. Capture provides neither arbitrary dependency closure nor model,
real-media, GPU execution, Handy, platform, human or release qualification.

The follow-up [shared capture custody increment](PULSAR_NATIVE_CAPTURE_CUSTODY.md) replaces the helper-specific reaper with shared worker custody, separates CPU/storage charges and verifies a worker-to-image staging handoff. Its tracked constructor uses the caller's committed engine scope and durable staging; actual native tests cover image-owned lease retention after engine drop. Production GPU launch remains disabled, and that constructor still awaits its production GPU caller. The earlier measurement packet above retains its own source and runtime scope.
