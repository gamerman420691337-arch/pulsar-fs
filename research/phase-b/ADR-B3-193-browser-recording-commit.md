# ADR B3-193: Bind browser commitment to a reviewed proposal and original intent

Bind explicit master replacement to the reviewed candidate, base revision and
program digest, returning a retained commit identity before dispatch. A stage
response alone does not express commitment, and replacing a request after lost
acknowledgement could authorize a different effect; separate review, preparation
and exact retry preserve that boundary without manufacturing review dispositions
or physical acceptance.

See [contract and evidence](../../docs/PULSAR_BROWSER_RECORDING_COMMIT.md).
