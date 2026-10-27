# Bounded piped encoded streams

`live-capture stream PROJECT REVISION EPOCH` accepts an ongoing encoded producer on standard input for a previously admitted encoded capture lease. The producer must use the explicitly admitted YUV4MPEG2, NUT or Matroska format and original source timestamps. Pulsar does not launch or acquire the producer through this command.

The Unix client duplicates and owns an unbuffered input descriptor without changing shared open-file flags. Pipes and bounded regular-file stdin are accepted. Interactive, device and socket descriptors refuse before native source attachment. At most one 65536-byte chunk is retained in the client; the existing single-attachment authenticated channel and confined source worker own exact byte/hash/frame admission and publication. Source bytes are neither staged as a complete file nor replayed after ambiguous I/O.

Polling is bounded to 10 milliseconds. Fresh authenticated status reconciliation occurs at most every 250 milliseconds while waiting for producer bytes. Initial status is capped at ten seconds; subsequent status calls are capped at one second and the conservative absolute lease end. Status, native attachment and every bulk exchange use absolute transport deadlines. The deadline uses the beginning of the status exchange plus reported remaining lease time, so reply transit time cannot extend the local end. Later observations can only tighten it. Equal-rate local monotonic clocks remain an assumption; engine checks and the independent native watchdog retain actual effect/worker authority.

EOF explicitly seals intake and waits for exact native completion. Empty stdin, read/poll failure, oversized intake, malformed decoding, stop, scope loss and expiry fail without implicit reconnect or rearm. The reader consumes at most one byte beyond the admitted input to detect excess, and never forwards that excess. A producer that keeps its pipe open does not keep the client alive beyond the lease. Client retirement can precede the actual server end by the conservative bracket, so withdrawal may be recorded as worker failure rather than server expiry; no physical stop or response is inferred from either.

The SDK adds absolute-deadline capture-status/encoded-attachment helpers and a monotone `LiveEncodedClient::limit_deadline` fence. Native frame/inference reports retain decoded-stdout delivery age; pipe arrival does not become camera exposure evidence.

Authored native CLI scenarios exercise three decoded frames before producer EOF, exact completion after EOF, idle stop and idle expiry with producer input still open, empty input, oversized input without forwarding, malformed decoding and refusal of a device descriptor before source claim. These are functional stream/confinement cases, not hardware acquisition or latency qualification.

Camera/stream acquisition adapters, automatic original-PTS segment production, sustained epoch orchestration, measured exposure/player clocks and gaps, player buffering, GPU/six-axis/model quality, physical scheduling and Handy/platform/human/release qualification remain required. Training and sterile evaluation are unchanged.

Evidence: [retained validation](evidence/roadmap-2026-10-01/piped-encoded-streams/validation.json).
