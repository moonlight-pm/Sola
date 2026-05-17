{
  description = "Sola — a Wayland desktop shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      packages.${system}.sola = pkgs.callPackage ./nix/sola.nix { };
      packages.${system}.default = self.packages.${system}.sola;
      nixosModules.default = import ./nix/module.nix;
    };
}
