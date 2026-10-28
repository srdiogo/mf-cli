# Add MiniFuse 4 status, verified mixer/routing control and configuration snapshots

MiniFuse 4 support currently exposes INST and phantom power, while its internal
mixes require another operating system's control application. This change adds
readable status, per-source level/pan/mute/solo, masters, captured source
membership and routing for all three MF4 mixes. It also adds host-side snapshot
save/show/diff/restore with dry-run plans and durable operation backups.

The existing MF1/MF2/MF4 toggle syntax and model-specific selectors are retained.
The new B2/mixer/config device operations are restricted to MF4 PID `af70`;
neither the MF1/MF2 state layout nor per-input MF2 INST is inferred from MF4.

The kernel transport gains a fixed, versioned read/ioctl ABI, validates complete
plans and rollback values, checks expected state/device identity, detects short
transfers, and verifies full-state readback. It attempts and verifies rollback
after failed writes. The legacy text ABI remains limited to the established
toggles. udev access uses the active local session instead of a world-writable
node, with rule ordering before systemd's seat access step. Both DKMS and Nix
include the new protocol header.

Evidence and protocol documentation credit the previous MF4 work and include
the sanitized 712-write extraction. The implementation corrects the out12
USB-direct polarity and documents independent addressing of the three mixes.

Validation: Rust tests (including C/Rust validator and ioctl conformance over
all selectors), capture-backed model tests, snapshot/journal failure tests,
CLI offline checks, formatting, Clippy and a kernel-module build. The fork has
not been loaded or exercised on hardware. Audible validation cited in the
evidence document belongs to the earlier reference implementation only.

Known boundaries: no calibrated meters, trim writes, analog mono/stereo
conversion, `56` activation, LEDs or copy-mix policy. Restores apply only mapped
writable fields and report omitted differences; opaque bytes and trims retain
their live values. A verified readback does not guarantee completion of device
nonvolatile saving. The change adds no firmware-update or device-storage writer.
