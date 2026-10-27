# ADR B3-071: Terminal evidence worker failure

Status: Implemented; scoped regression and native positive-path verification passed.
Date: 2026-10-08

## Problem

Polling only successful channel receives silently treats a disconnected worker as an empty queue. If the worker disappears while a request is outstanding, the panel remains busy forever and cannot explain whether a file export completed.

## Decision

Distinguish empty from disconnected reply channels. Disconnection clears busy state, invalidates disposable records and destination drafts, and marks the controller unavailable until the desktop window is reopened. Disable UI requests; also handle disconnected enqueue with the same terminal path. Keep full-queue failure retryable. Do not infer publication success or absence: explicitly instruct the user to inspect any requested destination before retrying. Project/revision navigation cannot reactivate this dead controller.

No Engine, protocol, database, bulk transfer or device behavior changes. The existing native desktop-worker original-byte campaign remains the positive regression path; channel-failure injection exercises the terminal UI reducer.

## Validation and limits

Inject a disconnected worker while busy, verify cleared busy/data/draft state and refused subsequent work, navigate to another revision and construct the disabled egui panel. Repeat the native positive worker campaign and ordinary workspace gates. Actual isolated-window interaction could not run because the required Agent Workspace tools are unavailable; headless output is not window or human usability qualification. Retention/product/model/media/Handy/platform/human/release gates remain open.
