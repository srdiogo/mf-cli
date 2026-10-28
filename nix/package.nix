{
  lib,
  rustPlatform,
  pkg-config,
  libusb1,
}:

rustPlatform.buildRustPackage {
  pname = "mf-cli";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../tests
      ../docs/mf4
      ../kmod/mf4_protocol.h
      ../70-minifuse.rules
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ libusb1 ];

  postInstall = ''
    install -Dm644 70-minifuse.rules $out/lib/udev/rules.d/70-minifuse.rules
  '';

  meta = {
    description = "CLI tool and seamless kernel module for Arturia MiniFuse 1/2/4";
    homepage = "https://github.com/nolight132/mf-cli";
    license = lib.licenses.mit;
    mainProgram = "mf-cli";
    platforms = lib.platforms.linux;
  };
}
