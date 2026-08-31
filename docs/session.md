# Driver-owned radio sessions

`RadioSession` is the client-facing execution layer for issue [#20](https://github.com/nicksbar/rigwright/issues/20).
It wraps one `Radio` connection and owns the command worker, refresh cadence,
state reconciliation, and recovery status.

Clients submit intent and await a `SessionTicket`. The session provides:

- bounded admission with explicit `QueueFull` backpressure;
- validation against driver capabilities before a command reaches the radio;
- coalescing of pending frequency, mode, PTT, refresh, and per-control work;
- safety-priority PTT commands ahead of ordinary state updates;
- desired, observed, and pending state in every `RadioSnapshot`;
- worker-owned refresh polling and ingestion of driver event-router updates;
- snapshot events and `Starting`, `Ready`, `Degraded`, and `Stopped` status.

The session does not replace vendor drivers. Protocol framing, model ranges,
control encoding, serial ownership, and profile-specific baud lists remain in
the driver/profile boundary. `RadioModelProfile::supported_baud_rates()` and
`fastest_supported_baud_rate()` expose documented connection choices so a
client can present the fastest suitable option. Automatic probing is a future
transport concern because it must be negotiated before the session can issue
normal CAT traffic.

This layer is Rigwright-only. QSONaut and QSONoid are not changed by the issue
#20 implementation, and no shared modem repository is required.
