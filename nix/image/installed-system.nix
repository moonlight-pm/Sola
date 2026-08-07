# Post-install system profile — what lands on the target disk after apply.
#
# Partition labels must match sola-install-apply (SOLA-ESP + sola-root).
# Username is written at apply time to /etc/sola/install-user (mutable).

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
    ./quiet-boot.nix
    ./installed-session.nix
  ];

  networking.hostName = "sola";
  networking.useDHCP = lib.mkDefault true;
  networking.networkmanager.enable = false;

  # Interim fixed zone until auto-detect lands (freeze: auto-detect later).
  time.timeZone = "America/Denver"; # US/Mountain

  console.keyMap = "us";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb = {
    layout = "us";
    variant = "mac";
  };

  # Labels set by sola-install-apply when formatting the target disk.
  fileSystems."/" = {
    device = "/dev/disk/by-label/sola-root";
    fsType = "ext4";
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/SOLA-ESP";
    fsType = "vfat";
    options = [
      "fmask=0077"
      "dmask=0077"
    ];
  };

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.timeout = 0;

  boot.kernelModules = [ "virtio_gpu" ];

  # End-user is created at apply time (mutable).
  users.mutableUsers = true;
  # Keep a recovery root with empty password for engineering only (serial).
  users.users.root.initialHashedPassword = "";

  security.sudo.wheelNeedsPassword = false;

  fonts.packages = with pkgs; [
    inter
    jetbrains-mono
  ];

  environment.systemPackages = with pkgs; [
    pciutils
  ];

  system.stateVersion = "25.05";
}
