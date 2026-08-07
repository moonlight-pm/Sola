# Shared live-media settings for installer qcow and ISO.
# No disk layout / image format here — callers add disk-image or iso-image.

{
  lib,
  pkgs,
  ...
}:

{
  networking.hostName = "sola";
  networking.useDHCP = lib.mkDefault true;
  networking.networkmanager.enable = false;

  # Live media clock; installed system sets America/Denver (interim).
  time.timeZone = "UTC";

  console.keyMap = "us";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb = {
    layout = "us";
    variant = "mac";
  };

  security.sudo.wheelNeedsPassword = false;

  fonts.packages = with pkgs; [
    inter
    jetbrains-mono
  ];

  environment.systemPackages = with pkgs; [
    pciutils
  ];

  boot.kernelModules = [ "virtio_gpu" ];

  system.stateVersion = "25.05";
}
