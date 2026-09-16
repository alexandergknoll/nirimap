{
  description = "A minimap overlay for the Niri Wayland compositor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        nirimap = pkgs.callPackage ./nix/package.nix { };
        default = nirimap;
      });

      overlays.default = final: _prev: {
        nirimap = final.callPackage ./nix/package.nix { };
      };

      # Home Manager is the right layer for nirimap: it is a per-user session
      # program, not a system service.
      homeModules = rec {
        nirimap = import ./nix/home-manager.nix self;
        default = nirimap;
      };
      # Deprecated alias, kept for configurations still using the old name.
      homeManagerModules = self.homeModules;

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.nirimap ];
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
          ];
        };
      });

      checks = forAllSystems (pkgs: {
        inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) nirimap;
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
