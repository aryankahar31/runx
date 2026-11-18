{
  description = "runx — run projects with their declared runtime versions";

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
        runx = self.packages.${pkgs.system}.default;
      });

      # Use from a NixOS configuration / home-manager:
      #   inputs.runx.url = "github:aryankahar31/runx";
      #   environment.systemPackages = [ inputs.runx.packages.${system}.default ];
      overlays.default = final: prev: {
        runx = final.callPackage ./package.nix { };
      };
    };
}
