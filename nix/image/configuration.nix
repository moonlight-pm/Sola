# Sola image profile — quiet branded boot + installer kiosk (QEMU qcow harness).
#
# QEMU dogfood: `cargo make vm build` / `vm run` / `vm install`.

{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:

{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
    (modulesPath + "/virtualisation/disk-image.nix")
    ./live-common.nix
    ./quiet-boot.nix
    ./installer-session.nix
  ];

  image.baseName = "sola-vm";
  image.format = "qcow2";
  image.efiSupport = true;

  virtualisation.diskSize = 20480; # MiB
}
