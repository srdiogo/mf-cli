{
  lib,
  stdenv,
  kernel,
  kernelModuleMakeFlags,
}:

let
  kdir = "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build";
in
stdenv.mkDerivation {
  pname = "minifuse-mod";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../kmod;
    fileset = lib.fileset.unions [
      ../kmod/Makefile
      ../kmod/minifuse_mod.c
      ../kmod/mf4_protocol.h
    ];
  };

  nativeBuildInputs = kernel.moduleBuildDependencies;

  makeFlags = kernelModuleMakeFlags ++ [
    "KDIR=${kdir}"
  ];

  hardeningDisable = [
    "pic"
    "format"
  ];

  enableParallelBuilding = true;

  # kbuild's modules_install runs the kernel's signing hook, which writes the
  # minifuse_mod.ko.cer that NixOS needs when it aggregates the module tree.
  installPhase = ''
    runHook preInstall
    make -C ${kdir} M="$PWD" ${lib.escapeShellArgs kernelModuleMakeFlags} \
      INSTALL_MOD_PATH="$out" modules_install
    runHook postInstall
  '';

  meta = {
    description = "Kernel transport for MiniFuse toggles and verified MiniFuse 4 mixer controls";
    homepage = "https://github.com/nolight132/mf-cli";
    license = lib.licenses.gpl2Only;
    platforms = lib.platforms.linux;
  };
}
