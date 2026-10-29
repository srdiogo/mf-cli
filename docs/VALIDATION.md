# Validation record

Local validation on 28 October 2026:

| Check | Result |
|---|---|
| Rust 1.85.1 (minimum supported compiler series) | Build succeeds |
| Unit tests | 22 pass |
| CLI integration tests | 3 pass, offline only |
| C/Rust selector and ioctl conformance | Pass; 65,536 selectors × 14 values |
| Capture-backed state-model test | Pass; 712 rows, 686 exposed / 26 excluded |
| Formatting and Clippy with warnings denied | Pass |
| Optimized release build | Pass |
| Kernel module, `W=1` | Builds for 7.2.5-3-omarchy |
| Evidence/fixture SHA-256 | Pass |
| PKGBUILD shell syntax and local documentation links | Pass |

The release CLI and kernel module were also exercised on one MF4 (`1c75:af70`)
using a temporary module load. Status, all three mixes, routes, request
rejection, backup failure and a 20-command snapshot restore were checked.
Digital loopback measurements confirmed gain/pan/mute/solo/master behavior and
USB-direct/custom routing. Each completed test phase restored all 190 B2 bytes.
See the [hardware validation report](mf4/hardware-validation.md) for the tested
commit, build hashes, method, measured results and coverage limits.

The [CI workflow](https://github.com/srdiogo/mf-cli/actions/workflows/ci.yml)
runs Rust minimum/stable checks and a separate kernel-header build.
Nix expressions were updated for the additional source/header/test files;
Nix and a complete AUR package build were not available locally.

MF1/MF2 hardware toggle regressions, multiple-device behavior, disconnect
recovery and power-cycle persistence remain untested. Prior reference
implementation results are distinguished from this fork's tests in
[the evidence record](mf4/evidence.md).
