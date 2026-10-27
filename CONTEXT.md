# Pulsar

Language for Pulsar's motion proposals, live lifetimes and retained resource obligations.

**Archival CPU image declaration**:
The historical description of the expected CPU library image for one original
execution attempt. Preserving it does not install its libraries, qualify its
results or authorize another attempt.
_Avoid_: Qualified backend, executable runtime bundle, resume permission

**Combined playback clock**:
A time correspondence whose accepted error budget includes both host/server and device timing
for one selected setup. It retains the validity limits of its original observations.
_Avoid_: Server timestamp, fresh synchronization, playback permission

**Synchronization custody**:
The retained obligation to account for an authorized device-clock synchronization attempt
until completion or unclaimed cancellation is established. It does not establish clock accuracy.
_Avoid_: Synchronized playback, accepted timing, active controller

**Device timing observation**:
A report of device uptime, reported clock offset, latest-sync round-trip delay and user timing
offset. It does not establish when synchronization occurred or how accurate it is.
_Avoid_: Fresh synchronization, calibrated device clock, accepted timing bound

**Server-clock observation**:
A measured correspondence between host time and the manufacturer's playback clock under
explicitly reviewed error and origin assumptions. It does not establish device synchronization
or the presentation time of a browser frame.
_Avoid_: Device synchronization, browser synchronization, playback permission

## Language

**Ownership scope**:
The persistent identity under which Pulsar accounts for its managed work and temporary assets. Scope identity alone is not permission to act.
_Avoid_: Engine epoch, capture epoch

**Capture epoch**:
One authorized live-capture lifetime with its own time correspondence and freshness boundary.
_Avoid_: Ownership scope, process lifetime

**Staging custody**:
Pulsar's retained responsibility for temporary captured bytes and their admitted capacity until their use and cleanup are confirmed complete.
_Avoid_: Best-effort deletion, released staging

**Creation intent**:
A retained declaration that an owned temporary resource may be created and must be reconciled before its capacity can be released.
_Avoid_: Created resource, successful capture

**Cleanup obligation**:
Outstanding staging custody after work ends, while deletion remains unconfirmed.
_Avoid_: Completed retirement, cancellation acknowledgement

**Retirement tombstone**:
A retained record of ended resource use whose cleanup and final clearing remain to be confirmed.
_Avoid_: Active worker, removed resource


**Recording source**:
A retained collection of accepted capture estimates with their original timing and evidence. It supplies material for motion authoring without asserting physical qualification.
_Avoid_: Funscript, committed motion, playable media

**Motion candidate**:
A motion proposal whose selected sources and evidence remain available for review before an explicit commitment to the project timeline.
_Avoid_: Accepted master, qualified motion, capture recording


**Candidate staging intent**:
A retained choice of recordings, timing range and identity for one motion proposal. Repeating that choice refers to the same proposal rather than authorizing a replacement.
_Avoid_: Motion candidate, commitment, creation intent


**Authored gap fill**:
A chosen construction of motion over explicitly missing intervals. It does not turn missing source evidence into observed or inferred evidence.
_Avoid_: Recovered observation, qualified correction

**Camera presentation time**:
The declared time of a camera image in the browser's presented video stream. It
does not establish when the camera exposed the image or its correspondence to a
physical device clock.
_Avoid_: Camera exposure time, calibrated device time

**Transport observation**:
A captured acknowledgment or unknown outcome for an issued command. Recording it
does not establish that a physical device executed the command or stopped.
_Avoid_: Physical stopping evidence, playback authorization


**Device registration**:
A retained owner configuration identifying credential references for a logical device.
It does not establish current connectivity, physical identity or playback authority.
_Avoid_: Qualified device, live device session, playback admission


**Handy connection session**:
An owner's temporary reservation of a registered Handy's credential route and its
connection observation. It does not grant playback authority or exclusive physical control.
_Avoid_: Qualified device, playback session, physical connection guarantee

**Connection observation**:
A captured report of connectivity and device identity. It is not a continuous liveness
claim, timing qualification or evidence of physical stopping.
_Avoid_: Live device guarantee, qualification receipt, stopped device


**Prepared Stroke script**:
A local publication of one committed revision's Stroke samples, identified by its exact
bytes and quantization report. Preparation grants neither cloud disclosure nor device playback.
_Avoid_: Loaded device program, qualified trajectory, hosted script


**Archival export scope**:
The subject of a recorded file publication: either one selected motion axis or a whole
analytic device-motion proposal. It describes historical content, not playback authority.
_Avoid_: Device axis, qualified device program


**Hosting observation**:
A historical result of explicitly disclosing an exact prepared script to temporary public
hosting. Verified content does not establish a currently usable location or device admission.
_Avoid_: Loaded script, playback grant, qualified motion


**Setup inspection**:
A read-only comparison of current device observations and the exact hosted script selected
for a project revision. It does not associate the remote stream with that script or admit playback.
_Avoid_: Loaded script, setup approval, physical stopping evidence


**Qualification dossier**:
A retained exact subject and set of evidence bytes offered for review of one device and
control/profile combination. Presence and content integrity do not establish physical truth.
_Avoid_: Accepted qualification, live admission, manufacturer certificate

**Qualification acceptance**:
An explicit local operator decision accepting the reviewed physical evidence for its bound
subject. It is separate from current script admission, physical stopping and playback authority.
_Avoid_: Imported evidence, automatic approval, setup performed


**Handy sample envelope**:
The permitted position range, adjacent position change and time interval of the quantized
samples selected for a Handy. It does not describe motion between samples or prove physical dynamics.
_Avoid_: Physical safety proof, interpolated trajectory, playback permission


**Handy operative settings**:
The explicit slider configuration and stroke limits observed on the device and bound to
reviewed evidence. Matching settings establish configuration correspondence, not physical motion or stopping.
_Avoid_: Model defaults, assumed calibration, physical qualification


**Setup intent**:
An exact device, configuration, script and hosting selection awaiting a setup handoff.
It is separate from loaded-program correspondence and playback authority.
_Avoid_: Loaded script, playback admission

**Setup claim**:
A one-use reservation for attempting a particular setup handoff. It does not establish
delivery, successful loading or physical stopping.
_Avoid_: Setup acknowledgment, physical proof, reusable dispatch permission


**Handy credential route**:
The route identified by the exact captured connection key within a private owner-key namespace.
It is separate from a logical registration and from physical-device identity across key changes.
_Avoid_: API key, physical serial number, loaded-program authority


**Setup acknowledgment**:
A transport response to a captured setup attempt that passed the local protocol checks.
It establishes neither complete loaded-program correspondence nor physical stopping.
_Avoid_: Loaded script, playback admission, stopped proof

**Unclaimed setup cancellation**:
The release of an exact setup preparation before any handoff claim has been consumed.
It preserves the original history and establishes no physical stopping or remote retraction.
_Avoid_: Stop command, attempted-setup reconciliation, erased history

**Handy stream association**:
A sampled correspondence between a current stopped stream and one acknowledged setup under
the selected program and device configuration. It does not establish complete device contents or playback authority.
_Avoid_: Content hash, loaded-program proof, physical readiness

**Handy playback preparation**:
A retained, nonplaying selection of a program, device configuration and accepted qualification
awaiting live-controller and timing authority. A historical receipt does not restore that selection.
_Avoid_: Armed device, live controller, playback permission

**Handy controller lease**:
A bounded, renewable period of exclusive local controller ownership for a prepared program.
It is separate from paired-client authentication and does not establish physical playback or stopping.
_Avoid_: Paired session, permanent controller, physical readiness


**Effect reconciliation**:
A resolution of outstanding device-command custody using fresh qualified evidence while preserving the original commands and outcomes.
_Avoid_: Cancellation, replay, restored playback, edited history

**Recovery contract**:
A physically qualified declaration of when prior commands have settled and fresh stopped observations establish quiescence for an exact device configuration.
_Avoid_: Transport acknowledgment, assumed stopping delay, operator override

**Explicit resumption**:
A new user-authorized playback attempt with fresh configuration, controller and timing admission after prior uncertainty has been resolved.
_Avoid_: Automatic restart, receipt replay, reconciliation


### Ongoing Handy timing

**Reported script phase**:
The device's reported time position within its selected script stream. It is separate
from physical motor position and the presentation time of a browser frame.
_Avoid_: Physical position, source-player time, exact execution time

**Original play reference**:
The script-time and server-time correspondence retained for one admitted play, together
with its original uncertainty. Later observations do not improve that historical uncertainty.
_Avoid_: Refreshed start, current phase, new synchronization

**Continuous timing observation**:
A bounded observation of reported script phase and server time for an existing playing
stream, interpreted under an accepted phase-to-execution error contract.
_Avoid_: Clock synchronization, physical position proof, playback permission

**Continuous timing admission**:
Temporary authority to retain the timing of an existing play using a current qualified
observation and uninterrupted correspondence to its original play reference.
_Avoid_: Resumption, renewed original synchronization, historical receipt

**Timing handoff**:
Replacement of a current timing admission while its predecessor remains valid. It does
not bridge an interval in which no applicable timing admission remained valid.
_Avoid_: Restart, gap repair, receipt replay

**Play alignment error bound**:
The bound accounting for observed deviation from the original play reference and the
retained uncertainties of that reference, current reported phase and current server time.
_Avoid_: Latest network round trip, motor-position error, improved original uncertainty


**Browser player identity**:
One browser launch, page, playback and source-selection lifetime. It identifies a selection epoch, not immutable media bytes or a source-player timeline.
_Avoid_: Imported source identity, physical device identity, synchronized playback

**Player stop guard**:
A bounded native attachment that retires an existing acknowledged play when its selected browser player stops or loses liveness.
_Avoid_: Playback permission, synchronized start, physical stopping proof

**Player challenge**:
A one-use native probe whose response demonstrates participation by an exact browser player within a bounded interval.
_Avoid_: Frame presentation timestamp, calibrated clock, reusable heartbeat

**Media progress deadline**:
The independent native deadline measured from the last accepted advancing frame count and media timestamp of a player.
_Avoid_: Heartbeat freshness, inference-result freshness, controller lifetime


**Prepared browser source**:
A finite native read relationship to exact imported bytes, an active source placement and
declared clock anchors. It may exist before playback and while playback is paused.
_Avoid_: Playing source, calibrated source clock, RGB-origin attestation

**Declared browser/source clock**:
An explicit affine relation between browser presentation time and native elapsed-source
time, composed with the selected placement to interpret submitted timestamps.
_Avoid_: Measured display time, first-frame calibration, synchronization authority

**Browser execution bracket**:
The native issue-to-complete-ingress interval of a one-use probe answered from a
fresh browser frame callback. It bounds the reported sample's execution, not the
time that pixels became visible.
_Avoid_: Half round-trip latency, display measurement, calibrated clock offset

**Browser timing observation**:
A process-local association between an execution bracket, one exact player/source
binding and the declared project phase of its reported frame. It expires and
cannot be reconstructed from a historical report.
_Avoid_: Synchronized start, qualified motion, reusable playback permission

**Browser/source timing qualification**:
Reviewed measurement bounds for one browser environment and its exact source,
placement and prepared script, dependent on a separately accepted device contract.
_Avoid_: Browser identity inferred from User-Agent, hardware-only qualification

**Referenced browser start**:
An original prepared-script phase and device server time chosen together under
the accepted browser/source timing bounds. Delayed delivery does not change the pair.
_Avoid_: Live generated-motion streaming, physical acknowledgment, renewed start

**Browser playback owner**:
The lifetime responsible for one original Start and its cancellation or stop,
including the period before an effect is admitted and any uncertain outcome afterward.
_Avoid_: Proof of movement, reusable start permission, historical playback authority

**Generated Handy stream**:
A sequence of timed stroke points added to one Handy stream as analysis produces
them, with a separate lifetime from a complete hosted script.
_Avoid_: Hosted-script playback, recorded inference, automatically synchronized output

**Append outcome**:
The known acknowledgment or unresolved result of one exact submitted point chunk.
An observed cumulative tail does not identify the chunk's bytes or prove movement.
_Avoid_: Physical execution receipt, deduplication key, retry permission

**Stream coverage horizon**:
The end of continuous acknowledged motion available ahead of an independently
bounded playback phase. Missing coverage is not permission to invent a hold or bridge.
_Avoid_: Total stored point count, buffer capacity, current physical position

**HSP command claim**:
The single durable consumption of one original stream command before native
dispatch. It records custody even when the eventual delivery result is unknown.
_Avoid_: Physical execution receipt, renewed command permission, retry token

**Unclaimed cancellation proof**:
A native result that one exact HSP command was cancelled before any durable claim.
It permits retiring that command's slot without labelling it an uncertain delivery.
_Avoid_: Rejected-worker response, physical Stop acknowledgment, restored authority

**Unclaimed command custody**:
Pulsar's exclusive responsibility for one original HSP command that has not entered durable claim. Waiting under that custody supplies neither delivery evidence nor new command permission.
_Avoid_: Retry token, reconstructed command, uncertain delivery


**Finite input closure**:
The producer's declaration that no additional source frames will follow its exact final admitted frame, while existing analysis may still be unfinished.
_Avoid_: Stop, worker completion, final visible frame

**Input seal**:
Confirmation that the declared final input, its analysis and its delivery have settled with their original identities. It does not establish device acknowledgment or physical completion.
_Avoid_: Ingress receipt, end-of-file event, renewed source permission

**Finite-tail drain**:
The bounded completion of already acknowledged final motion after input is sealed, under the original valid playback permissions and an explicit terminal-behavior contract.
_Avoid_: Starvation, synthetic holding point, successful physical completion

**Tail passage**:
A conservative observation that the playback phase has reached the final acknowledged point. Physical completion and stopping remain separate observations.
_Avoid_: Buffer-end notification, final append receipt, recovery drain proof

**Source transfer**:
A verified change from one capture epoch to its successor while retaining the original motion stream. It preserves the real shared frame and each epoch's original observations.
_Avoid_: Worker readiness, restarted playback, renumbered samples

**Clock observation generation**:
One immutable set of timing observations with its own original validity. A later generation provides no additional lifetime to a command based on an earlier one.
_Avoid_: Renewed command deadline, synchronization acknowledgment, physical-position proof


**Fresh playback evaluation**:
An assessment of current playback eligibility at a time following selection of its
browser and playback-clock observations. Those observations retain their original
validity and supply no renewed synchronization or command deadline.
_Avoid_: Renewed observation, refreshed command reference, permission grace


## Browser HSP controls

**HSP source selection**:
An exact choice of analysis source, visible player, model/profile and qualification
references for a generated Handy stream. Inspecting that choice does not establish
current playback authority.
_Avoid_: Page-supplied stream authority, accepted motion, reusable playback permission

**Browser HSP control lifecycle**:
One visible player's retained preparation, stream-control and continuity choices
with their original outcomes. It remains unresolved until its owned work and cleanup
obligations are accounted for.
_Avoid_: Hosted-script Start, preparation readiness, physical playback confirmation

**Joined HSP lifecycle archive**:
A record of original operation identities after a browser HSP control lifecycle's
work and cleanup are confirmed settled. It supplies history without making a previous
stream or authorization current again.
_Avoid_: Resumed stream, restored authority, physical completion evidence

**Unsubmitted browser intent**:
An original browser HSP choice that ended before its native submission began. It
does not describe an attempted or uncertain command delivery.
_Avoid_: Unclaimed cancellation proof, failed acknowledgment, stopped device


**HSP certified passage**:
An in-memory lower bound already established within one unchanged forward-only
stream. Later uncertainty may weaken without undoing that passage; contradictory
fresh bounds still stop. It supplies no current source/clock authority and cannot
be restored by history. Future Stop projections remain separate from current
passage. See B5-068.
