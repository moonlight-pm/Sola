# Quiet branded boot — replace the firmware framebuffer ASAP.
#
# Timeline the user should see:
#   1. Brief firmware (OVMF/OEM) — we cannot paint this on arbitrary boards
#   2. Kernel + simpledrm claims the EFI GOP → Plymouth flower + spinner
#   3. Handoff to installer kiosk / Sola
#
# Logs go to serial only (engineering). The graphical surface stays on the
# brand splash — not a wall of systemd text over the BIOS screen.

{
  config,
  lib,
  pkgs,
  ...
}:

let
  solaPlymouth = pkgs.callPackage ./plymouth/default.nix { };
in
{
  boot.consoleLogLevel = 0;
  boot.initrd.verbose = false;
  boot.loader.timeout = lib.mkDefault 0;

  # systemd-in-initrd is what modern nixpkgs uses for early Plymouth.
  boot.initrd.systemd.enable = lib.mkDefault true;

  boot.kernelParams = [
    "quiet"
    "splash"
    "loglevel=0"
    "udev.log_level=0"
    "rd.udev.log_level=0"
    "systemd.show_status=false"
    "rd.systemd.show_status=false"
    "vt.global_cursor_default=0"
    # Critical with serial engineering logs: without this, Plymouth sees
    # console=ttyS0 and often *skips the graphical splash*, leaving the
    # OVMF/BIOS framebuffer on screen for the entire boot.
    "plymouth.ignore-serial-consoles"
    # Graphical VT exists for DRM/Plymouth; keep it quiet. Serial carries
    # engineering logs (host terminal with mon:stdio).
    "console=tty0"
    "console=ttyS0,115200n8"
    # Prefer modes that match cargo make vm run's virtio-vga geometry.
    "video=Virtual-1:1920x1080@60"
  ];

  boot.plymouth = {
    enable = true;
    # Custom theme: flower is the spinner (neon cyan petal cycle).
    theme = "sola";
    themePackages = [ solaPlymouth ];
    # Still set logo for anything that falls back to the stock watermark path.
    logo = "${solaPlymouth}/logo.png";
  };

  # simpledrm binds the EFI/GOP framebuffer the moment the kernel is up,
  # so Plymouth can paint *over* the leftover OVMF image without waiting
  # for full virtio-gpu bring-up. Load simpledrm first.
  boot.initrd.kernelModules = [
    "simpledrm"
    "virtio_pci"
    "virtio_gpu"
  ];

  boot.initrd.availableKernelModules = [
    "simpledrm"
    "virtio_pci"
    "virtio_blk"
    "virtio_scsi"
    "virtio_net"
    "virtio_gpu"
    "virtio_console"
  ];
}
