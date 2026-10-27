# ADR B3-136: Preceding image-origin reader boundary

Status: scoped preceding-snapshot campaign PASS; roadmap PARTIAL; unqualified.

Build a frozen pre-image-origin source snapshot from retained exact files and a locked dependency set, using distinct artifact names. Use the actual preceding client to accept a compatible package and refuse format 13, and the actual preceding application to refuse first/second clone states without changing committed SQLite main/WAL. Keep source reconstruction and released historical tags distinct.

Record source and executable hashes, exact refusal diagnostics, current lifecycle results and cleanup. No archive, report, model or device identity grants execution authority. A scoped preceding-snapshot pass does not close broader historical-reader, model/media, platform/device, human or release gates.

Implementation and evidence: [preceding-reader scope](../../docs/PULSAR_IMAGE_PRECEDING_READER.md).
