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
    # Fill with `nix build .#packaging.nix` output. Nix prints the value in
    # the hash-mismatch error; replacing is a one-time step per release.
    hash = "__SOURCE_HASH__";
  };

  # Same one-time step: the first build prints the expected cargoHash.
  cargoHash = "__CARGO_HASH__";

  meta = with lib; {
    description = "Run any project with the exact runtime versions it declares";
    homepage = "https://github.com/aryankahar31/runx";
    license = licenses.mit;
    mainProgram = "runx";
    platforms = platforms.unix ++ platforms.windows;
  };
}
