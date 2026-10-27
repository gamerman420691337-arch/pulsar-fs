# ADR B3-099: portable approved assistant asset paths

Status: implemented; scoped policy, library, lint and binary verification PASS. Scope: existing Linux assistant host policy.

Problem: schema-1 private assistant owner policy binds absolute runtime/model paths. Relocating an offline installation invalidates those paths even when the approved asset identities and limits are unchanged.

Decision: retain schema-1 behavior and introduce schema 2 with runtime/model paths relative to the explicitly selected policy file's canonical parent. Require nonempty normal relative components, canonical resolution equal to the joined path, and existing regular/executable metadata admission. Reject absolute paths, traversal, missing assets and symlinks. Capture the private policy using its existing bounded current-owner/single-link/no-follow stable-file checks. Keep explicit inference approval, pinned worker/runtime/model identities, deadlines, resource bounds and subsequent registration/capture hashing authoritative.

No cwd search, environment asset substitution, downloader, automatic approval or model-quality selection is added. Moving the installation requires selecting its moved policy path; the policy bytes and identities remain unchanged. Schema 1 stays absolute-only; unknown schemas refuse. Registration consumes resolved absolute paths through the existing asset capture path.

This supports a relocation component of M6; actual offline Core/Complete artifacts, installed default selection, OS/vendor matrix, real learned quality and human acceptance require independent evidence. Canonical path checking is not a filesystem isolation boundary; later exact-byte capture and confined execution remain required.
