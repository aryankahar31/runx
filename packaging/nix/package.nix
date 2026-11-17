{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:

rustPlatform.buildRustPackage rec {
  pname = "runx";
  version = "__VERSION__";

  src = fetchFromGitHub {
    owner = "aryankahar31";
    repo = "runx";
    rev = "v${version}";
    # Release-template placeholder; fill from the `nix build .#default`
    # hash-mismatch output for this tagged source version.
    hash = "__SOURCE_HASH__";
  };

  # Release-template placeholder; fill from the following build's cargoHash
  # mismatch output after replacing the source hash above.
  cargoHash = "__CARGO_HASH__";

  meta = with lib; {
    description = "Run any project with the exact runtime versions it declares";
    homepage = "https://github.com/aryankahar31/runx";
    license = licenses.mit;
    mainProgram = "runx";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ];
  };
}
