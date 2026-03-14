{
  description = "A framework to design sound, reproducible and scalable mining repositories studies on GitHub.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
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

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              curl
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              libiconv
            ];
            doCheck = false;

            meta = with pkgs.lib; {
              description = desc;
              mainProgram = pname;
              platforms = platforms.unix;
            };
          };
        });
    };
}
