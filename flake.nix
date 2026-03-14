{
  description = "A framework to design sound, reproducible and scalable mining repositories studies on GitHub.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.package.version;
      pname = cargoToml.package.name;
      desc = cargoToml.package.description;
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            inherit pname version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            meta = with pkgs.lib; {
              description = desc;
              mainProgram = pname;
              platforms = platforms.unix;
            };
          };
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/${pname}";
          meta = {
            description = desc;
          };
        };
      });
    };
}
