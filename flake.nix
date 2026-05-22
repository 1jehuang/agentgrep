{
  description = "CLI-first code search and retrieval for agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        agentgrep = pkgs.rustPlatform.buildRustPackage {
          pname = "agentgrep";
          version = "0.1.2";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
          meta = {
            description = "CLI-first code search and retrieval for agents";
            homepage = "https://github.com/1jehuang/agentgrep";
            license = pkgs.lib.licenses.mit;
            mainProgram = "agentgrep";
          };
        };
      in
      {
        packages.default = agentgrep;
        apps.default = {
          type = "app";
          program = "${agentgrep}/bin/agentgrep";
        };
      }
    );
}
