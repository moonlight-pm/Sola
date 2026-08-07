# Tools + apply helper for the live installer image.
#
# Expects `installSystem` via specialArgs (the sola-installed toplevel path).

{
  config,
  lib,
  pkgs,
  installSystem,
  ...
}:

let
  applyPath = lib.makeBinPath (
    with pkgs;
    [
      coreutils
      util-linux
      gptfdisk
      parted
      e2fsprogs
      dosfstools
      nixos-install-tools
      nix
      gnugrep
      gnused
      gawk
      kmod
      systemd # udevadm
    ]
  );
  applyScript = pkgs.writeShellScriptBin "sola-install-apply" ''
    export PATH="${applyPath}:$PATH"
    ${builtins.readFile ./sola-install-apply.sh}
  '';
  rebootScript = pkgs.writeShellScriptBin "sola-install-reboot" ''
    exec ${pkgs.systemd}/bin/systemctl reboot
  '';
in
{
  # Ensure the target system closure is present in the installer image.
  system.extraDependencies = [ installSystem ];

  environment.etc."sola/install-system".text = "${installSystem}\n";

  environment.systemPackages = [
    applyScript
    rebootScript
  ]
  ++ (with pkgs; [
    nixos-install-tools
    gptfdisk
    parted
    e2fsprogs
    dosfstools
    util-linux
    nix
  ]);

  # Wizard runs as sola-install; apply + reboot need root.
  security.sudo.extraRules = [
    {
      users = [ "sola-install" ];
      commands = [
        {
          command = "${applyScript}/bin/sola-install-apply";
          options = [
            "NOPASSWD"
            "SETENV"
          ];
        }
        {
          command = "${rebootScript}/bin/sola-install-reboot";
          options = [ "NOPASSWD" ];
        }
      ];
    }
  ];
}
