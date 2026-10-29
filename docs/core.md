# Shared MiniFuse control library

`crates/mf-core` is the synchronous Rust library used by `mf-cli`. It owns
model capabilities, state decoding, gain conversion, domain planning, the
existing kernel transport and durable host snapshots. It does not start a
service, load a module, detach an audio driver or choose a desktop application.

## Public API

An external consumer can inspect a snapshot and propose an operation offline:

```rust
use mf_core::{Level, Mix, Operation, Snapshot};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
let saved = Snapshot::load(Path::new("studio.json"))?;
let before = saved.state()?;
let proposed = mf_core::plan(&saved.device, &before, &[
    Operation::Master { mix: Mix::Out12, level: Level::Db(-12.0) },
])?;
assert_eq!(proposed.before(), &before);
// Planning never opens a device. A live operation must use freshly read state.
Ok(())
}
```

The four domain operations are `Channel`, `Master`, `Route` and `Membership`.
`Level::Silence` is distinct from a finite dB value. An absent channel field
preserves it. Overlapping selectors are rejected before unchanged fields are
filtered, including overlaps with membership removal's mute/solo effects.

`Plan` cannot be constructed or edited by consumers. It binds the USB device
record, complete initial payload, bounded changes and predicted final payload.
`plan_restore` accepts a validated schema-1 snapshot and an optional mix scope;
`omitted_offsets` reports bytes outside that restore scope. `summary` retains
the existing CLI diagnostic JSON, but it is not an execution input format.
There is no arbitrary-selector or raw-state-write public method.

`Kernel::open(false)` and `read` provide confirmed state. Execution uses a
write-capable kernel handle and `execute(&mut backend, &plan, backup_directory)`.
No userspace USB fallback is used for mixer operations. `Backend` provides the
same typed transaction boundary to deterministic tests and the real kernel.
The kernel remains the final full-state/device-identity guard against external
writers; a library plan does not grant exclusive ownership of the interface.

## Errors and execution outcomes

`ErrorKind` distinguishes invalid input, unsupported capability, access,
ambiguous/missing device, missing/incompatible module, changed identity, stale
state, backup, transfer, readback and journal failures. Transport classification
uses operating-system errors and kernel flags, never parsed terminal wording.
`device_write_attempted = None` means dispatch cannot be established reliably.

Preparation errors return before a device mutation. After dispatch, `Outcome`
preserves the device result, confirmed state if available, rollback attempted
and verified flags, journal status, backup and operation paths. A rollback is
verified only when its flag and complete returned state agree with the original
payload. An attempted rollback is not a successful recovery.

A verified device write can coexist with `JournalStatus::Failed`. The durable
prepared record remains available and must not be replayed. `require_success`
is the terminal adapter: it reports that combination as `JournalFailed` while
retaining its confirmed device state. Service/UI callers should inspect the
separate outcome fields instead of collapsing them into one boolean.

## Compatibility and dependencies

The workspace retains Rust 1.85, CLI syntax/exit behavior, legacy model maps,
snapshot schema 1, exclusive mode-0600 files, pre-write sync and the existing
206-byte status/1032-byte transaction ABI. Kernel sources and control allowlists
are unchanged. Journals retain their existing status and fields, with additive
device-outcome/recovery fields. Opaque state, trims, topology restrictions and
unvalidated controls retain their previous behavior.

MF1/MF2 advertise no mixer/snapshot capability and never receive inferred B2
queries. Switch readback, meters and physical-button events remain unavailable.
Capabilities describe potentially supported membership; the planner also checks
the actual topology. Device readback still does not prove nonvolatile saving.

The crate is not published to crates.io. Workspace consumers use a path
dependency; independent repositories must pin a reviewed Git revision and
package `mf-core`. Its only dependencies are Serde, serde_json and libc from
the committed lockfile. Clap and libusb remain in the CLI package.

## Validation

`cargo test --workspace --locked` runs the preserved CLI/model/fixture and
C/Rust conformance tests, plus a consumer compiled outside the library. The
consumer covers immutable planning, model gates, overlap rejection, recovery
facts and journal-finalization failure after verified execution. CI also runs
formatting, Clippy, release builds and the independent kernel build on the
minimum Rust toolchain and current stable where applicable.

These offline checks exercise the software boundary. The existing hardware
report remains tied to its recorded source/binary hashes; extraction alone
does not constitute another hardware test.
