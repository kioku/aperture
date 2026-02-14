{
  description = "Aperture – Dynamic CLI generator for OpenAPI specifications";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;

      mkAperture =
        pkgs:
        { pname ? "aperture", features ? [ ] }:
        pkgs.rustPlatform.buildRustPackage rec {
          inherit pname;
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

          src = pkgs.lib.cleanSource ./.;

          cargoLock.lockFile = ./Cargo.lock;

          buildFeatures = features;

          # Tests require network access (wiremock) which is unavailable
          # in the Nix build sandbox.  They are validated separately in CI.
          doCheck = false;

          meta = {
            description = "Dynamic CLI generator for OpenAPI specifications";
            homepage = "https://github.com/kioku/aperture";
            license = pkgs.lib.licenses.mit;
            mainProgram = "aperture";
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          build = mkAperture pkgs;
        in
        {
          default = build { };
          aperture-jq = build {
            pname = "aperture-jq";
            features = [ "jq" ];
          };
          aperture-openapi31 = build {
            pname = "aperture-openapi31";
            features = [ "openapi31" ];
          };
          aperture-full = build {
            pname = "aperture-full";
            features = [
              "jq"
              "openapi31"
            ];
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
            ];
          };
        }
      );
    };
}
