# ADR B3-211 — Desktop ownership of the shared browser player

Status: Implemented software; qualification PARTIAL
Date: 2026-10-13
Architecture: ARCH158

## Problem

The local browser player was accessible only through a command requiring an existing project ID, revision and options file. Pulsar Desktop already owns the current project selection, but did not provide a launch or server-ownership workflow. A second browser generator or a subprocess with unobserved shutdown would duplicate state or leave capture ownership unclear.

## Decision

Add a Browser live desktop panel. Capture its selected project/revision and explicit bounded options-file path, and run the existing browser adapter in one owned worker. Share validation, loopback HTTP, recording/recovery and capture cleanup with CLI. A readiness callback publishes only the bound loopback URL; native credentials stay in SessionClient. Launch does not dispatch StartLiveCapture.

The worker has a cancellation flag checked before startup publication and before dispatching a parsed HTTP operation. Stop, project/revision changes, finite expiry and owner destruction request the existing capture Stop path. One worker is retained until actual termination and join; a second launch refuses while it is owned. Ordinary window close defers exit and displays the Browser live panel while cleanup is pending. Destruction outside that path cancels and joins directly.

## Consequences and limits

An accepted operation in flight finishes under its original identity; cancellation is not rollback or an Engine transport interrupt. Existing RPC/bulk bounds apply, so shutdown can wait. Failed Stop remains an error, abrupt process death still relies on leases, and unknown effects are not adopted. Browser-side commits do not replace the desktop snapshot; current Engine revision checks still decide subsequent effects. No public device capability or model default changes.

The actual native controller tests use this production panel and a private real Engine, with explicit authored temporal inference. Native thread ownership and headless controls do not prove rendered window-close behavior, desktop/browser platform usability, useful motion, source-player timing, Handy synchronization or release acceptance. Scope and source-bound evidence are in [the desktop browser contract](../../docs/PULSAR_DESKTOP_BROWSER_LIVE.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-13/desktop-browser-launch/checkpoint.json).
