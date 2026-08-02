{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
        libPath = with pkgs; lib.makeLibraryPath [
          libGL
          libxkbcommon
          wayland
          vulkan-loader
        ];
      in
      {
        defaultPackage = naersk-lib.buildPackage ./.;
        devShell = with pkgs; mkShell {
          buildInputs = [
            cargo rustc rustfmt pre-commit rustPackages.clippy 
            cargo-watch cargo-machete
            wasm-pack # wayland pkg-config

          ];
          RUST_LOG = "debug";
          LD_LIBRARY_PATH = libPath;
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
        };
      }
    );
}
