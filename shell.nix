{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  packages = with pkgs; [
    rustc
    cargo
    pkg-config
    gcc

    qt6.qtbase
    qt6.qtdeclarative
    qt6.qttools
  ];

  shellHook = ''
    export QT_PLUGIN_PATH="${pkgs.qt6.qtbase}/${pkgs.qt6.qtbase.qtPluginPrefix}"
    export QMAKE="${pkgs.qt6.qtbase.dev}/bin/qmake"
  '';
}
