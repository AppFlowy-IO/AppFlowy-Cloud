{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  packages = with pkgs; [
    pkg-config rustup
    grpc-tools openssl
  ];
  shellHook = ''
    export PROTOC=$(which protoc)
  '';
}
