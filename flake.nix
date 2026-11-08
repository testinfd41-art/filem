{
  description = "Filem — a quiet, keyboard-first file manager for Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # qmetaobject discovers Qt through one include directory and one library
      # directory. Nix gives each Qt module its own store path, so merge the
      # modules we use into a single prefix.
      qtBuild = pkgs.buildEnv {
        name = "filem-qt-build";
        paths = with pkgs.qt6; [
          qtbase
          qtbase.dev
          qtdeclarative
          qtdeclarative.dev
          qtmultimedia
          qtwayland
          qtsvg
        ] ++ [
          pkgs.libglvnd
          pkgs.libglvnd.dev
        ];
        ignoreCollisions = true;
      };

      qtRuntime = pkgs.buildEnv {
        name = "filem-qt-runtime";
        paths = with pkgs.qt6; [
          qtbase
          qtdeclarative
          qtmultimedia
          qtwayland
          qtsvg
        ] ++ [
          pkgs.libglvnd
        ];
        ignoreCollisions = true;
      };

      commonEnv = {
        QMAKE = "${qtBuild}/bin/qmake";
        QT_INCLUDE_PATH = "${qtBuild}/include";
        QT_LIBRARY_PATH = "${qtRuntime}/lib";
        QML2_IMPORT_PATH = "${qtRuntime}/lib/qt-6/qml";
        QML_IMPORT_PATH = "${qtRuntime}/lib/qt-6/qml";
        QT_PLUGIN_PATH = "${qtRuntime}/lib/qt-6/plugins";
        LD_LIBRARY_PATH = "${qtRuntime}/lib:${pkgs.glib}/lib";
      };

      filem = pkgs.rustPlatform.buildRustPackage {
        pname = "filem";
        version = "0.1.0";

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            let name = baseNameOf path; in
              !(builtins.elem name [ "target" ".qt-dev" ".direnv" "result" ]);
        };

        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = [
          pkgs.pkg-config
          pkgs.qt6.wrapQtAppsHook
        ];
        nativeCheckInputs = [
          pkgs.ffmpeg
          pkgs.imagemagick
          pkgs.poppler-utils
          pkgs.p7zip
          pkgs.file
          pkgs.exiftool
          pkgs.glib.bin
          pkgs.gvfs
          pkgs.udisks
          pkgs.util-linux
          pkgs.systemd
          pkgs.bzip2
          pkgs.xz
          pkgs.zstd
          pkgs.shared-mime-info
        ];
        buildInputs = [
          pkgs.qt6.qtbase
          pkgs.qt6.qtdeclarative
          pkgs.qt6.qtmultimedia
          pkgs.qt6.qtwayland
          pkgs.qt6.qtsvg
          qtRuntime
          pkgs.glib
          pkgs.glib.dev
        ];

        env = commonEnv;

        qtWrapperArgs = [
          "--prefix QT_PLUGIN_PATH : ${qtRuntime}/lib/qt-6/plugins"
          "--prefix QML2_IMPORT_PATH : ${qtRuntime}/lib/qt-6/qml"
          "--prefix QML_IMPORT_PATH : ${qtRuntime}/lib/qt-6/qml"
          "--prefix LD_LIBRARY_PATH : ${qtRuntime}/lib"
          "--prefix LD_LIBRARY_PATH : ${pkgs.glib}/lib"
        ];

        preCheck = ''
          export XDG_CACHE_HOME="$TMPDIR/filem-cache"
          mkdir -p "$XDG_CACHE_HOME"
          export XDG_DATA_DIRS="${pkgs.shared-mime-info}/share:${pkgs.gvfs}/share"
        '';

        postInstall = ''
          mkdir -p $out/share/filem
          cp -r qml $out/share/filem/qml
          install -Dm644 packaging/filem.desktop $out/share/applications/filem.desktop
          install -Dm644 packaging/filem.png \
            $out/share/icons/hicolor/512x512/apps/filem.png
        '';

        meta = {
          description = "A quiet, keyboard-first file manager for Linux";
          license = pkgs.lib.licenses.mit;
          mainProgram = "filem";
          platforms = [ "x86_64-linux" ];
        };
      };
    in
    {
      packages.${system} = {
        default = filem;
        filem = filem;
        qt-runtime = qtRuntime;
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rustfmt
          pkgs.clippy
          pkgs.patchelf
          pkgs.pkg-config
          pkgs.ffmpeg
          pkgs.imagemagick
          pkgs.libnotify
          pkgs.poppler-utils
          pkgs.p7zip
          pkgs.file
          pkgs.exiftool
          pkgs.glib.bin
          pkgs.glib
          pkgs.glib.dev
          pkgs.gvfs
          pkgs.udisks
          pkgs.util-linux
          pkgs.bzip2
          pkgs.xz
          pkgs.zstd
        ];

        inherit (commonEnv) QMAKE QT_INCLUDE_PATH QT_LIBRARY_PATH
          QML2_IMPORT_PATH QML_IMPORT_PATH QT_PLUGIN_PATH LD_LIBRARY_PATH;

        shellHook = ''
          export FILEM_QML_DIR="$PWD/qml"
          echo "Filem development shell"
          echo "  Run:   cargo run"
          echo "  Test:  cargo test"
          echo "  QML:   $FILEM_QML_DIR"
        '';
      };
    };
}
