{
  # The dev shell of the checks (AGENTS.md, Change workflow). What `make install` builds and
  # installs needs none of it: only cargo, herdr-app and systemd.
  inputs = {
    # The pinned Herdr tools, shared by the apps on Herdr. nixpkgs, rust-overlay and cccc follow
    # its lock, so this shell reuses the store paths of herdr-common's.
    herdr-common.url = "github:flupke/herdr-common";
    nixpkgs.follows = "herdr-common/nixpkgs";
    rust-overlay.follows = "herdr-common/rust-overlay";
    cccc.follows = "herdr-common/cccc";
  };

  outputs =
    {
      cccc,
      herdr-common,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
    in
    {
      devShells = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          # The herdr-app and herdr-core (`hc`) releases the panel test runs, whatever is
          # installed: herdr-common pins them for every app on Herdr (Linux only).
          pinned = herdr-common.packages.${system};
          latestRustPlatform = pkgs.makeRustPlatform {
            cargo = pkgs.rust-bin.stable.latest.default;
            rustc = pkgs.rust-bin.stable.latest.default;
          };
          rustComplexityAnalyzer = latestRustPlatform.buildRustPackage {
            pname = "cccc-cli";
            version = "1.6.0";
            src = cccc;
            cargoLock = {
              lockFile = "${cccc}/Cargo.lock";
              outputHashes."tree-sitter-kotlin-0.4.0" =
                "sha256-O9zCo8G8321cpqVp4z8USwyoLtnJReNhWs1FpYa9IVQ=";
            };
            cargoBuildFlags = [ "--package=cccc-cli" ];
            cargoTestFlags = [ "--package=cccc-cli" ];
            buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.glibc.dev ];
            "BINDGEN_EXTRA_CLANG_ARGS_${pkgs.stdenv.hostPlatform.rust.rustcTarget}" =
              pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
                "-isystem ${pkgs.glibc.dev}/include"
              );
            nativeBuildInputs = [
              pkgs.git
              pkgs.rustPlatform.bindgenHook
            ];
          };
        in
        {
          default = pkgs.mkShell (
            {
              packages = [
                pkgs.rust-bin.stable."1.90.0".default
                pkgs.cargo-nextest
                pkgs.jq
                pkgs.typos
                rustComplexityAnalyzer
              ]
              # The panel test's app sandbox (herdr-common's crates/app-sandbox) runs bubblewrap,
              # and enters it with nsenter.
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                pkgs.bubblewrap
                pkgs.util-linux
              ];
            }
            // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
              # Not on PATH: `herdr-app` and `hc` there stay the user's.
              TEST_HERDR_APP_BIN_PATH = "${pinned.herdr-app}/bin/herdr-app";
              TEST_HC_BIN_PATH = "${pinned.hc}/bin/hc";
              # The private herdr-app's display: a headless Weston, which starts its Xwayland.
              TEST_WESTON_BIN_PATH = "${pkgs.weston}/bin/weston";
              # Mesa's software Vulkan driver: the private herdr-app draws on the CPU, the same on
              # every machine, and never opens the user's GPU.
              TEST_HERDR_APP_VULKAN_DRIVER = "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.uname.processor}.json";
            }
          );
        }
      );
    };
}
