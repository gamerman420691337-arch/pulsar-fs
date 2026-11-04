# Corrupt video decoding failures

2026-10-12. Scoped authored MPEG-4 corruption regression PASS; roadmap PARTIAL. No real-media, model, device, platform or release qualification.

The production preview adapter previously returned a successful frame from a damaged video packet. FFmpeg reported codec errors but concealed them and exited successfully. The same default decoding could emit all 16 frames from the damaged authored control, giving callers no terminal failure.

The shared native video command now supplies `-xerror`. Preview refuses the reproduced damaged inputs without publishing a frame. The streaming decoder may deliver a valid prefix, but its existing child-status check returns a terminal error when the codec fails. This change preserves source clocks and the existing frame layout. It does not detect every possible corruption or alter the separate audio decoder.

The permanent `scripts/qualify-corrupt-media.py` reproducer authors a two-second 64×64 MPEG-4 control at eight frames per second and damages packet ordinals 1, 3 and 7 using recorded packet positions. It records exact tool/input hashes and default versus strict decoder outcomes. Default FFmpeg exits zero and emits 16 frames despite error diagnostics; strict decoding exits 183 after 1, 3 and 7 frames respectively. All media is synthetic and owned; no reference scripts, annotation labels or hardware are involved.

The opt-in Engine regression uses the actual production preview and shared generation streaming adapter. The clean control produces its requested preview and all 16 correctly timed frames. Each damaged input refuses preview, leaves its output directory empty, and reports a streaming failure after the corresponding prefix. Rational timestamps are compared by value without changing their native time base. This is adapter coverage, not a whole-application candidate-publication or GUI acceptance campaign.

Evidence: [checkpoint](evidence/roadmap-2026-10-12/corrupt-media/checkpoint.json). The default Engine library suite passes 832 tests with 69 ignored; the new ignored corruption campaign passes separately. Both strict workspace all-target lint configurations, workspace all-target check, formatting, 431 architecture checks, 36 source guards, and App/Worker build are recorded in the packet. Other library/feature/platform runtime suites were not rerun here.

Retained failures include the original successful-looking damaged preview, an initial missing PathBuf qualification in the test fixture, and the initial structural rational-time assertion. The latter compared unequal timestamp representations of equal times and was corrected to cross-multiply. Broader corrupt codecs/containers, audio, real media, model quality, distribution, hardware and human acceptance remain open.
