# Exact offline build blocker

The first historical campaign stopped during dependency resolution, before Rust
compilation or execution of any original test. Cargo exited 101; the campaign
runner exited 2. All twenty cases are unavailable, not passed or failed assertions.

The local offline Cargo resolver reported:

```text
error: failed to select a version for the requirement `serialport = "^4.10"`
  version 4.10.0 is yanked
location searched: crates.io index
```

This is the observed local-index result, not a fresh online registry verification.
The preserved manifest requests serialport 4.10. No original legacy Cargo.lock was
preserved, and this failed resolution generated no experimental lock receipt.
No package version was upgraded, downloaded, manually unlocked, or replaced.
No historical production function or original test assertion was changed.

Original fixture SHA-256:
`1ed992a14efd8479a770703a6da848d1cc2562b86142cbb9c169af59751fa68b`

Preserved baseline aggregate SHA-256 (ordered full-file manifest in subject.json):
`04f242a0562983cc6e9efeefd7384809cf8d7e03f20eed72274c0169cff64bbb`

Both remained unchanged after the failed attempt. Exact appended modules, test
identities, isolation manifest, raw resolver output and per-case related-current
coverage are retained in this campaign. Category correspondence is deliberately
not represented as current test execution or one-to-one historical closure.

A future authorized recovery would need a defensible dependency resolution,
preferably the actual historical lock and available matching crates. It must
remain a new campaign with its own subject/dependency receipts. This artifact
neither authorizes network access nor proposes changing the preserved baseline
merely to obtain a green build. B1 and the whole Good phase remain incomplete.
