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
    in {
      packages.${system} = {
        sola = pkgs.callPackage ./nix/sola.nix { };
        river-patched = river-patched;
        default = self.packages.${system}.sola;
      };

      # Pass our flake-evaluated river-patched into the module so it
      # uses the right version even when imported into a configuration
      # that itself uses a different nixpkgs (e.g. stable).
      nixosModules.default = { config, lib, pkgs, ... }@args:
        import ./nix/module.nix (args // {
          riverPackage = river-patched;
        });
    };
}
