{
  description = "stealthbar: Turtle Beach Stealth Pro II status for status bars (Noctalia widget included)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "stealthbar";
          version = (fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          # The udev rule for services.udev.packages, and the Noctalia plugin for a plugin source of kind "path".
          postInstall = ''
            install -Dm644 70-stealthbar.rules -t $out/lib/udev/rules.d
            mkdir -p $out/share/noctalia/plugins
            cp -r plugin $out/share/noctalia/plugins/stealthbar
          '';
          meta = {
            description = "Battery and mic status of a Turtle Beach Stealth Pro II headset as JSON lines";
            license = pkgs.lib.licenses.mit;
            mainProgram = "stealthbar";
            platforms = pkgs.lib.platforms.linux;
          };
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
          ];
        };
      });

      formatter = forAll (pkgs: pkgs.nixfmt-tree);
    };
}
