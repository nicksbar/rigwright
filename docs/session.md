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

## Performance and safety behaviors

These behaviors are what make the driver feel fast and trustworthy on a live
radio. They are on by default and need no client wiring.

### Batched core-state reads (fewer round trips)

`Radio::read_core_state()` returns frequency, mode, and PTT in as few
protocol round trips as the backend allows. The modern Yaesu driver answers
frequency and mode from the single `IF;` frame and PTT from `TX;`, so a full
core refresh costs two round trips instead of three (`FA;`/`MD;`/`TX;`). It
falls back to the individual reads when `IF;` is unavailable. The session
`Refresh` operation uses this automatically.

### Trusting a live event stream (free refreshes)

CI-V radios push unsolicited frequency/mode/PTT events. When
`Radio::event_stream_age()` reports a fresh stream and the session already
holds observed core state, a `Refresh` is served from that streamed state
instead of re-polling the wire. A healthy Icom link therefore refreshes
without extra CAT traffic; a stalled stream falls back to polling. This is
why a streaming radio feels instant.

### Optimistic state on writes

A successful `set_frequency`/`set_mode`/`set_ptt` updates the observed state
immediately, before any confirmation poll, and the radio's own event echo
confirms it. Combined with coalescing, dragging a VFO issues only the latest
intent.

### Link health and scope keep-alive

`Radio::link_health()` returns a protocol-neutral `LinkHealth` (commands,
matched/timeout responses, consecutive-timeout backlog, mean latency, dropped
frames) with an `is_degraded()` heuristic, so an app can render "radio link
degraded: 3 consecutive timeouts" instead of a bare error. For Icom scopes,
`IcomCiVRadio::scope_stream_health()` reports sweep cadence and an
`is_stalled()` signal so the UI can re-arm a frozen waterfall.

### Retained-frame recovery

Both serial drivers retain recent unsolicited/out-of-order frames and match
them against later queries, so a reply that arrives slightly late (or
interleaved with scope data) is still used instead of timing out. This is a
large part of why cross-vendor behavior feels bulletproof.

### PTT safety watchdog

`SessionConfig::max_tx_hold` (default 180s) bounds any continuous transmit
hold. When the ceiling elapses the worker issues `SetPtt(false)` directly,
bypassing the command queue, and publishes `SessionEvent::PttWatchdogTripped`.
This protects the radio and operator if the client crashes or stalls while
transmitting. Set it to `None` to disable.

This layer is Rigwright-only. QSONaut and QSONoid are not changed by the issue
#20 implementation, and no shared modem repository is required.
