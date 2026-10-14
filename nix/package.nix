{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  gtk4-layer-shell,
}:

rustPlatform.buildRustPackage {
  pname = "nirimap";
  version = (lib.importTOML ../Cargo.toml).package.version;

  # Only the inputs the build actually reads, so edits to the README or CI
  # config do not invalidate the build.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    # Standard wrapper for GTK apps: sets up the runtime environment GTK
    # expects (GSettings schemas, pixbuf loaders, XDG_DATA_DIRS).
    wrapGAppsHook4
  ];

  buildInputs = [
    gtk4
    gtk4-layer-shell
  ];

  meta = {
    description = "A minimap overlay for the Niri Wayland compositor";
    homepage = "https://github.com/alexandergknoll/nirimap";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "nirimap";
  };
}
