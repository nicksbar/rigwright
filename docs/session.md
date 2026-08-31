# Driver-owned radio sessions

`RadioSession` is the client-facing execution layer for issue [#20](https://github.com/nicksbar/rigwright/issues/20).
It wraps one `Radio` connection and owns the command worker, refresh cadence,
state reconciliation, and recovery status.

The snapshot also carries a monotonically increasing session `generation` and
an explicit `synchronized` flag. Call `advance_generation()` after replacing
or reconnecting the underlying device; queued work from the previous
generation is completed with `StaleGeneration`, and in-flight results cannot
overwrite the new observed state.

Clients submit intent and await a `SessionTicket`. The session provides:

- bounded admission with explicit `QueueFull` backpressure;
- validation against driver capabilities before a command reaches the radio;
- coalescing of pending frequency, mode, PTT, refresh, and per-control work;
- safety-priority PTT commands ahead of ordinary state updates;
- desired, observed, and pending state in every `RadioSnapshot`;
- worker-owned refresh polling and ingestion of driver event-router updates;
- snapshot events and explicit `Opening`, `Probing`, `Synchronizing`,
  `Ready`, `Recovering`, `Degraded`, `Closing`, and `Stopped` status values;
- raw protocol operations that preserve queue order and are never coalesced;
- `SessionEvent::OperationAccepted` makes queue admission observable;
- `SessionEvent::OperationCompleted` records applied, superseded, stale, and
  failed work, while admission errors are reported as rejected outcomes;
- `SessionDiagnostics` counters for admission, completion, failure,
  coalescing, recovery, and generation changes.

A successful ticket resolves to the reconciled snapshot; stale, superseded, or
backend-failed work resolves with a typed `SessionError`. `reconnect()` swaps
the backend and its unsolicited-event source, increments the generation, and
invalidates queued work from the previous connection before new work runs.
Queued work has a bounded wait deadline and reports `TimedOut` before it reaches
the radio; vendor transport I/O deadlines remain owned by the driver.

The session does not replace vendor drivers. Protocol framing, model ranges,
control encoding, serial ownership, and profile-specific baud lists remain in
the driver/profile boundary. `RadioModelProfile::supported_baud_rates()` and
`fastest_supported_baud_rate()` expose documented connection choices so a
client can present the fastest suitable option. Automatic probing is a future
transport concern because it must be negotiated before the session can issue
normal CAT traffic.

This layer is Rigwright-only. QSONaut and QSONoid are not changed by the issue
#20 implementation, and no shared modem repository is required.
