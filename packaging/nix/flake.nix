{
  description = "runx-launcher — run projects with their declared runtime versions";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: {
        default = pkgs.callPackage ./package.nix { };
        runx-launcher = self.packages.${pkgs.system}.default;
      });

      # Use from a NixOS configuration / home-manager:
      #   inputs.runx.url = "github:aryankahar31/runx";
      #   environment.systemPackages = [ inputs.runx.packages.${system}.runx-launcher ];
      overlays.default = final: prev: {
        runx-launcher = final.callPackage ./package.nix { };
      };
    };
}
