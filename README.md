# mf-cli

A Linux command-line utility and kernel transport for Arturia MiniFuse audio
interfaces, written in Rust and C. Existing commands toggle instrument mode,
phantom power and Direct Mono where supported. This development branch adds
MiniFuse 4 mixer status, verified mixing/routing changes and configuration files.

The new functionality is **unreleased**, based on upstream v0.2.6. It is backed
by Windows captures, a previous Linux reference implementation and tests of
this Rust fork on one MiniFuse 4. See [evidence and validation](docs/mf4/evidence.md)
and the [hardware validation report](docs/mf4/hardware-validation.md).

## Compatibility

| Capability | MiniFuse 1 | MiniFuse 2 | MiniFuse 4 |
|---|---|---|---|
| USB product IDs | `af80`, `af82` | `af90` | `af70` |
| Existing `inst`, `48v`, `monitor` syntax | Preserved | Preserved | `monitor` unavailable |
| Independent `inst1` / `inst2` | No | Not added by this branch | Yes |
| B2 status, mixer, routes and live config commands | Unsupported | Unsupported | Implemented |
| Userspace USB fallback | Legacy toggles only | Legacy toggles only | Legacy toggles only |

Vendor ID is `1c75`. MF1 `af82` retains upstream's existing handling; this branch
adds no initialization or mode-transition command. The new mixer backend only
queries PID `af70`; it does not assume shared MF1/MF2 layouts. Upstream's open
[MF2 per-input INST PR #7](https://github.com/nolight132/mf-cli/pull/7) is separate.

## Build and install this branch

Requires Rust 1.85+, a C compiler, pkg-config, libusb development files, make
and headers for the target Linux kernel.

```sh
cargo build --release --locked
cargo test --workspace --locked
make -C kmod W=1
```

The tests need a C compiler for the kernel/userspace validation probe. They do
not open USB devices, load a module or send hardware controls.

To install the reviewed build:

```sh
sudo install -m755 target/release/mf-cli /usr/local/bin/mf-cli
sudo install -m644 70-minifuse.rules /etc/udev/rules.d/70-minifuse.rules
sudo udevadm control --reload-rules
sudo make -C kmod install
sudo modprobe minifuse_mod
```

If an older `minifuse_mod` is loaded, unload it with `sudo modprobe -r
minifuse_mod` before loading the new one, with no client holding its node open.
Remove an older manually installed `99-minifuse.rules` so it cannot override
the new permissions. Reconnect the interface after updating rules. The updated
CLI and module are required together for the new commands.

The `70-` rule tags the USB device and `/dev/minifuse_cmd` for active-local-session
access before systemd's seat-access rule. The node has mode 0660; it is not
world-writable. Headless/remote users need an administrator-assigned ACL or
group policy. Secure Boot may require signing the external module using the
system's existing signing mechanism. This project does not change boot policy.

AUR releases remain upstream releases until this work is accepted and published;
installing the released AUR package will not install this branch's new features.

### NixOS

Use this fork and branch as the flake input:

```nix
{
  inputs.mf-cli = {
    type = "github";
    owner = "srdiogo";
    repo = "mf-cli";
    ref = "master";
  };
  outputs = { nixpkgs, mf-cli, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        mf-cli.nixosModules.default
        { hardware.minifuse.enable = true; }
      ];
    };
  };
}
```

Disabling `hardware.minifuse.kernelModule.enable` leaves only the legacy
userspace fallback available. The new status/mixer/config device commands
require the module. Nix expressions are updated here but were not built locally.

## Existing toggles

```sh
# MiniFuse 1/2: original syntax and selector map
mf-cli inst on 48v on monitor off

# MiniFuse 4: instrument mode per input; shared phantom power for inputs 1/2
mf-cli inst1 on inst2 off 48v on
```

MF1/MF2 selectors remain INST=`0000`, +48V=`0400`, Direct Mono=`0500`.
MF4 uses INST1=`0000`, INST2=`0001`, +48V=`0300`. All send a two-byte boolean
payload using `21/22`, interface zero. The existing fallback can detach/reset
the device and interrupt audio. Legacy toggles refuse ambiguous multiple-device
setups; they are not snapshot-backed mixer transactions.

## MiniFuse 4 status and mixer

```sh
mf-cli status
mf-cli status --json
mf-cli mixer out12 channel analog1 --level -6 --pan 0.5
mf-cli mixer out34 channel usb2 --mute on --solo off
mf-cli mixer loopback master -6
mf-cli mixer out12 source analog2 on
mf-cli mixer loopback source usb56 off
mf-cli route out12 mix
mf-cli route out34 usb34
mf-cli route loopback out12
```

Mixes are `out12`, `out34`, `loopback`. Sources are `analog1`..`analog4` and
`usb1`..`usb4`; loopback additionally exposes `usb5`/`usb6`. Source inclusion
uses analog mono names or USB pair names (`usb12`, `usb34`, `usb56` where
available). Analog stereo conversion and USB pair inclusion on out34 remain
blocked. Existing out34 stereo sources still support gains, mute and solo.

Source level is -60..+12 dB or `-inf`; master is -64..0 dB or `-inf`. Pan is
0 (left)..1 (right). Level/pan are derived from bus gains, not live meters.
Changing mute/solo preserves those gains. A master/fader affects its custom
mix; it may not be in the selected output route when USB direct is active.

Append `--dry-run` to a mixer, route or restore command to inspect the plan
without writing USB. It still reads the live state. None of these commands
uses the legacy USB fallback, detaches `snd-usb-audio`, or resets the device.
Keeping the driver bound does not constitute a measured dropout/latency claim.

Every nonempty change saves a durable backup and prepared journal, validates
its selector/channel/value limits, compares the expected live state and
requires complete B2 readback. Failure triggers a best-effort, verified rollback
in the kernel; disconnection can prevent recovery. USB requests are sequential,
not atomic. Avoid concurrent control from other applications.

## Configuration files

```sh
mf-cli config save studio.json
mf-cli config show studio.json --json
mf-cli config diff studio.json
mf-cli config restore studio.json --dry-run
mf-cli config restore studio.json --mix out12

# Offline examples: no device node or USB access
mf-cli config show tests/fixtures/windows-state.json --json
mf-cli config diff tests/fixtures/windows-state.json \
  --against tests/fixtures/windows-state.json
```

Snapshots preserve the complete 190-byte state for inspection. Restoration
applies only captured writable mixer fields; opaque bytes, input trims and
other device settings are not replayed. Unsupported topology changes are
refused. Diff/dry-run output reports omitted byte offsets. Files never overwrite
an existing snapshot.

Backups default to `$XDG_STATE_HOME/mf-cli` or `$HOME/.local/state/mf-cli`.
Use `--backup-dir DIRECTORY` to override. A host snapshot and a verified
readback do not guarantee the interface has finished saving to nonvolatile
memory. No hardware save/flush command is added.

See [configuration and persistence](docs/mf4/configuration.md) for the schema,
restore scope, journaling and crash behavior.

## Shared library

The workspace includes the blocking `mf-core` library. The CLI and separate
[session service](https://github.com/srdiogo/mf-service) use the same device model, immutable plans, snapshot schema
and verified execution path. The core has no CLI parser, async runtime or GUI
dependency. See [the public API and compatibility contract](docs/core.md).

## Development and protocol reference

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
make -C kmod W=1
sha256sum -c docs/mf4/SHA256SUMS
```

- [Protocol and kernel ABI](docs/mf4/protocol.md)
- [Evidence, credits and hardware-validation boundaries](docs/mf4/evidence.md)
- [MiniFuse 4 hardware validation](docs/mf4/hardware-validation.md)
- [Configuration and device persistence](docs/mf4/configuration.md)
- [Local validation record](docs/VALIDATION.md)
- [Prepared PR description](docs/PR_DESCRIPTION.md)

The new work stays within known configuration controls. Signal meters, trim
writes, LED editing and mix-copy policy remain follow-ups. No arbitrary-command,
device-storage-writing or firmware-update interface is provided.

## License

Rust userspace code and helper material: [MIT](LICENSE).
Kernel module and its shared protocol header: [GPL-2.0-only](kmod/LICENSE).
See the evidence document for attribution to the upstream and MF4 research work.

## Standalone application and distribution preview

The [Qt application](https://github.com/srdiogo/minifuse-control) and
[session service](https://github.com/srdiogo/mf-service) are separate projects.
This repository owns the shared core and kernel transport; no GUI is embedded
in the CLI. MF4 input switches and C2 peak meters are documented in
[the input report](docs/mf4/input-switches.md) and
[the meter report](docs/mf4/meters.md).

A checksummed Arch preview recipe for the fork is maintained under
[packaging/arch](packaging/arch/README.md). It is separate from the upstream
release recipe at the repository root. The preview packages DKMS sources and
active-session access rules without reloading a running module, installing a
boot-load entry or enabling application/service startup.
