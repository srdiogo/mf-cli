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

The kernel module was compiled only. It was not installed or loaded, and no
control command was sent to the interface during this work. Release binaries
were used only for help and offline snapshot inspection/comparison.

CI defines Rust minimum/stable checks and a separate kernel-header build.
This document records local results, not an assertion that remote CI has run.
Nix expressions were updated for the additional source/header/test files;
Nix and a complete AUR package build were not available locally.

Hardware validation of this fork remains outstanding, including MF1/MF2 legacy
toggle regressions, MF4 status/control readback, audio behavior, disconnect
recovery and power-cycle persistence. Prior reference-implementation results
are documented separately in [the evidence record](mf4/evidence.md).
