# Arch platform transport preview

This recipe builds an immutable, checksummed source revision of the fork. It
packages the CLI, shared active-session udev rules and matching DKMS sources.
The `mf-cli-platform` package conflicts with the upstream `mf-cli` package;
review that replacement before installation. It is not an AUR release.

Build as an ordinary user with `makepkg -s`. Install with the system package
manager after reviewing the archive and dependencies. DKMS needs headers for
the intended kernel; an installed preview package does not validate a kernel
that was never built or loaded. Secure Boot signing uses the host's existing
policy. The CLI is MIT licensed; the module remains GPL-2.0-only.

The package does not reload a running module or install a boot-load entry.
After closing device clients, load the reviewed module with `modprobe
minifuse_mod`. If a previous module is already loaded, explicitly replace it
with no client holding the command node. Keep `snd-usb-audio` bound. Module
loading never belongs in the GUI or session service.

The optional boot-load example is installed under `/usr/share/doc`; copying it
to `/etc/modules-load.d/` is a separate administrator choice. Installing this
package does not enable GUI, service or desktop-agent startup. It does not
restore a device configuration or modify firmware.

The matching [service](https://github.com/srdiogo/mf-service) and
[GUI](https://github.com/srdiogo/minifuse-control) are independent projects.
Configuration, presets and operation evidence remain user-owned on removal.
