# ADR-B2-005: Explicit annotated temporal training views

Date: 2026-10-08
Status: Implemented staging capability; new model/runtime binding and qualification open.

The recorded whole-packed-source temporal candidate remains poor, while label and pixel/PTS reconstruction pass. Its 64×64 contract has no target or projection declaration. These observations justify making training inputs explicit, not attributing failure to a particular missing feature or changing an existing model's declared input.

Use the existing native `video_views` adapter for declared eye/projection planning. Factor its numeric dimension planning without changing existing generation or preview behavior. A bounded, read-only `pulsar training-input-plan JSON` role returns a versioned spatial recipe without Engine startup, media access or effects. Its request declares encoded source dimensions, maximum analysis dimensions, an existing `VideoView`, a static integer target rectangle in the resolved view and a bounded annotation identifier. Existing projection validation rejects ambiguous stereo/aspect/viewport declarations; region arithmetic is checked against resolved dimensions.

A separate stage-only CLI accepts an exact training-only manifest and source-hash-bound view annotations. It snapshots the annotation bytes, invokes a hash-bound native planner, applies eye/projection then region crop then nearest 64×64 resizing, and preserves original PTS/label admission. It writes schema-2 intake with per-entry plans and planner/annotation identities. Legacy staging remains schema 1. Legacy model export refuses schema-2 input; no existing model/profile is silently repurposed. No Engine client/worker RPC format or protocol schema changes.

The region is static, not a tracker or anatomical/physical ground truth. An annotation identifier does not prove target identity. Source/script alignment and resolution/cadence sufficiency remain unverified. The next required implementation is model/export/profile/runtime binding for this recipe, followed by permitted real annotations and independent quality evidence. Existing sterile inputs, datasets, reference scripts and models remain untouched. Undo consists of abandoning an exclusively created new private run; there is no activation or original-data rollback.

Verification must include native CLI staging of authored flat, full equirectangular and half-equirectangular stereo inputs; exact eye/region pixels; preserved PTS/labels; changed source identity and existing-output refusal; malformed native declarations; legacy-export refusal; and relevant workspace/architecture gates. Qualification and human/device/platform/release gates remain independent.
