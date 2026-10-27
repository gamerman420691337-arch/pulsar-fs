# ADR B5-040 — Observe device timing without inferring synchronization

Preserve the two device timing responses and their host brackets as unqualified observations.
The vendor reports neither last-sync age nor accuracy, so RTT cannot become an inferred clock
error or fresh synchronization grant. Keep the user offset separate from the reported clock
offset. Accept bounded signed integers to reconcile the GET endpoint's negative examples with
its contradictory nonnegative schema reference; refuse fractions instead of rounding.

Bind read-only inspection to an exact controller generation and verify the device context
around it. Failed post-capture inspection retires only that generation. Never add clocksync
to the read-only broker: its GET method initiates a device operation. Actual synchronization,
combined timing qualification and physical dispatch remain separate unfinished work.
