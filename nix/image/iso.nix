# Sola installer ISO — same live stack as the qcow harness, product media shape.
#
# Flake: packages.sola-iso / nixosConfigurations.sola-iso
# Build: cargo make iso build  (sets SOLA_VM_STAGE, impure)

{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:

{
  imports = [
    (modulesPath + "/installer/cd-dvd/iso-image.nix")
    # Virtio + QEMU guest for dogfood; quiet-boot already loads virtio_gpu.
    (modulesPath + "/profiles/qemu-guest.nix")
    ./live-common.nix
    ./quiet-boot.nix
    ./installer-session.nix
  ];

  # isoImage.isoBaseName was renamed to image.baseName on recent nixpkgs.
  image.baseName = lib.mkForce "sola";

  isoImage = {
    volumeID = "SOLA";
    makeEfiBootable = true;
    makeUsbBootable = true;
    # Squashfs: balance size vs build time for multi-GiB staged closure.
    squashfsCompression = "zstd -Xcompression-level 6";
    # Quiet GRUB menu — product wants short firmware → splash.
    appendToMenuLabel = "";
    prependToMenuLabel = "Sola ";
  };

  # ISO root is squashfs; do not inherit host filesystem layout.
  swapDevices = lib.mkImageMediaOverride [ ];
  boot.loader.timeout = lib.mkForce 0;

  # End-user accounts come from apply on the target disk only.
  # Live seat is sola-install (installer-session.nix).

  # Prefer our branded splash over any installer defaults.
  # (quiet-boot.nix sets plymouth + kernel params.)
}
