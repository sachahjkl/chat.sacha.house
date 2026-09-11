{
  description = "Single-room chat server";

  nixConfig = {
    extra-substituters = ["https://nix-community.cachix.org"];
    extra-trusted-public-keys = ["nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="];
  };

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2605";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    git-hooks = {
      url = "https://flakehub.com/f/cachix/git-hooks.nix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    bun2nix = {
      url = "github:nix-community/bun2nix?ref=2.1.2";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    flake-utils,
    crane,
    git-hooks,
    bun2nix,
    ...
  }:
    flake-utils.lib.eachSystem
    [
      "x86_64-linux"
      "aarch64-linux"
    ]
    (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [bun2nix.overlays.default];
        };
        craneLib = crane.mkLib pkgs;
        pname = "chat-sacha-house";
        version = "0.1.0";
        cleanSource = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            !builtins.elem (baseNameOf path) [
              ".git"
              ".jj"
              "dist"
              "node_modules"
              "result"
              "target"
            ];
        };
        frontSource = pkgs.lib.cleanSourceWith {
          src = ./front;
          filter = path: _type:
            !builtins.elem (baseNameOf path) [
              "dist"
              "node_modules"
            ];
        };
        bunDeps = pkgs.bun2nix.fetchBunDeps {bunNix = ./bun.nix;};
        frontend = pkgs.bun2nix.mkDerivation {
          inherit version bunDeps;
          pname = "${pname}-frontend";
          src = frontSource;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [pkgs.stdenv.cc.cc.lib];
          buildPhase = "bun run build";
          installPhase = "cp -r dist $out";
        };
        mkFrontCheck = name: command:
          pkgs.bun2nix.mkDerivation {
            inherit version bunDeps;
            pname = "${pname}-${name}";
            src = frontSource;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [pkgs.stdenv.cc.cc.lib];
            buildPhase = command;
            installPhase = "touch $out";
          };
        rustSource = pkgs.runCommand "${pname}-source" {} ''
          cp -r ${cleanSource} $out
          chmod -R u+w $out
          mkdir -p $out/front
          cp -r ${frontend} $out/front/dist
        '';
        cargoVendorDir = craneLib.vendorCargoDeps {src = cleanSource;};
        dummySrc = craneLib.mkDummySrc {src = cleanSource;};
        commonArgs = {
          inherit
            pname
            version
            cargoVendorDir
            ;
          cargoLockContents = builtins.readFile ./Cargo.lock;
          src = rustSource;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {inherit dummySrc;});
        server = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            doCheck = false;
          }
        );
        dockerImage = pkgs.dockerTools.buildLayeredImage {
          name = pname;
          tag = version;
          contents = [
            server
            pkgs.busybox
            pkgs.cacert
            pkgs.sqlite
            pkgs.tzdata
          ];
          fakeRootCommands = ''
            mkdir -p ./data
            chown 65532:65532 ./data
            chmod 0700 ./data
          '';
          config = {
            User = "65532:65532";
            Cmd = ["${server}/bin/chat_sacha_house"];
            Env = [
              "BIND_HOST=0.0.0.0"
              "BIND_PORT=3030"
              "DATABASE_URL=sqlite:/data/chat.db?mode=rwc"
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
            WorkingDir = "/data";
            Volumes."/data" = {};
            ExposedPorts."3030/tcp" = {};
          };
        };
        preCommitCheck = git-hooks.lib.${system}.run {
          package = pkgs.prek;
          src = ./.;
          hooks = {
            actionlint.enable = true;
            alejandra.enable = true;
            check-added-large-files.enable = true;
            check-merge-conflicts.enable = true;
            check-yaml.enable = true;
            end-of-file-fixer.enable = true;
            shellcheck.enable = true;
            trim-trailing-whitespace.enable = true;
          };
        };
        actionlint = pkgs.runCommand "${pname}-actionlint" {nativeBuildInputs = [pkgs.actionlint];} ''
          actionlint -config-file ${cleanSource}/.github/actionlint.yaml ${cleanSource}/.github/workflows/*.yml
          touch $out
        '';
      in {
        packages = {
          default = server;
          inherit dockerImage frontend;
        };

        checks = {
          inherit
            actionlint
            dockerImage
            frontend
            server
            ;
          pre-commit = preCommitCheck;
          cargo-fmt = craneLib.cargoFmt {src = cleanSource;};
          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );
          rust-tests = craneLib.cargoTest (commonArgs // {inherit cargoArtifacts;});
          svelte-typescript = mkFrontCheck "types" "bun run check";
          oxfmt = mkFrontCheck "oxfmt" "bun run format:check";
          oxlint = mkFrontCheck "oxlint" "bun run lint";
        };

        devShells.default = craneLib.devShell {
          packages =
            [
              pkgs.bun
              pkgs.bun2nix
              pkgs.alejandra
            ]
            ++ preCommitCheck.enabledPackages;
          shellHook = preCommitCheck.shellHook;
        };

        formatter = pkgs.alejandra;
      }
    );
}
