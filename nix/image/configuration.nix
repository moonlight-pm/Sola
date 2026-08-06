# Sola image profile — quiet branded boot + installer kiosk (product path).
#
# QEMU dogfood: `cargo make vm build` / `vm run` should show flower splash,
# then the kit installer (dry-run), not a wall of kernel text + getty.

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
    ./quiet-boot.nix
    ./installer-session.nix
  ];

  image.baseName = "sola-vm";
  image.format = "qcow2";
  image.efiSupport = true;

  virtualisation.diskSize = 20480; # MiB

  # console= / video= live in quiet-boot.nix so the graphical path stays
  # brand-splash-only; serial carries engineering logs.

  networking.hostName = "sola";
  networking.useDHCP = lib.mkDefault true;
  networking.networkmanager.enable = false;

  time.timeZone = "UTC"; # installer later overwrites via auto-detect on real apply

  # Mac keyboard / US English — locked product policy.
  console.keyMap = "us";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb = {
    layout = "us";
    variant = "mac";
  };

  # End-user accounts are created by the installer apply step.
  # The live media only has `sola-install` (see installer-session.nix).

  security.sudo.wheelNeedsPassword = false;

  fonts.packages = with pkgs; [
    inter
    jetbrains-mono
  ];

  environment.systemPackages = with pkgs; [
    pciutils
  ];

  # Extra virtio bits for the running system (initrd list is in quiet-boot.nix).
  boot.kernelModules = [ "virtio_gpu" ];

  # No /etc/issue wall of text — brand boot is Plymouth + installer UI.

  system.stateVersion = "25.05";
}
