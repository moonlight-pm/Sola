{
  description = "Sola — a Wayland desktop shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # River, patched and BUILT FROM OUR PINNED NIXPKGS. We can't
      # rely on the consumer's `pkgs.river` — on nixos-25.05 stable
      # that's 0.3.11 (the old wlroots river); our patch is for
      # river 0.4.x (the zig rewrite) which the pinned unstable has.
      # Computing it here lets the module reference the right version
      # regardless of the host system's nixpkgs channel.
      river-patched = pkgs.river.overrideAttrs (old: {
        patches = (old.patches or [ ])
          ++ [ ./nix/patches/river-xwayland-destroy-state.patch ];
      });

      # Package used inside image builds. When SOLA_VM_STAGE is set
      # (requires `nix build --impure`), install from the local stage
      # tree prepared by `cargo make vm build`. Otherwise fall back to
      # the GitHub release derivation (Shape 1).
      solaForImage =
        let
          stage = builtins.getEnv "SOLA_VM_STAGE";
        in
        if stage != "" then
          pkgs.callPackage ./nix/image/sola-from-stage.nix {
            stage = /. + stage;
          }
        else
          pkgs.callPackage ./nix/sola.nix { };

      solaNixosModule = { config, lib, pkgs, ... }@args:
        import ./nix/module.nix (args // {
          riverPackage = river-patched;
        });
    in {
      packages.${system} = {
        sola = pkgs.callPackage ./nix/sola.nix { };
        river-patched = river-patched;
        default = self.packages.${system}.sola;

        # Preinstalled qcow2 (EFI). Prefer building via `cargo make vm build`
        # so SOLA_VM_STAGE is set and the image carries current binaries.
        sola-vm-qcow2 =
          self.nixosConfigurations.sola-vm.config.system.build.image;
      };

      # Pass our flake-evaluated river-patched into the module so it
      # uses the right version even when imported into a configuration
      # that itself uses a different nixpkgs (e.g. stable).
      nixosModules.default = solaNixosModule;

      nixosConfigurations.sola-vm = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          ./nix/image/configuration.nix
          solaNixosModule
          {
            services.sola.enable = true;
            services.sola.package = solaForImage;
          }
        ];
      };
    };
}
