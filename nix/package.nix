{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  glib,
  gtk4,
  gtk4-layer-shell,
  wayland,
  adwaita-icon-theme,
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
    # nirimap resolves app icons through the GTK icon theme at runtime, which
    # needs XDG_DATA_DIRS, the GSettings schemas and the pixbuf loaders to be
    # baked into the binary's environment.
    wrapGAppsHook4
  ];

  buildInputs = [
    glib
    gtk4
    gtk4-layer-shell
    wayland
  ];

  # wrapGAppsHook4 only contributes the GSettings schema dirs to XDG_DATA_DIRS;
  # it does not pull in icon themes. Suffix a fallback theme so window icons
  # still resolve on a host that ships none, while leaving the session's own
  # XDG_DATA_DIRS (and therefore the user's icon theme) ahead of it.
  preFixup = ''
    gappsWrapperArgs+=(--suffix XDG_DATA_DIRS : "${adwaita-icon-theme}/share")
  '';

  meta = {
    description = "A minimap overlay for the Niri Wayland compositor";
    homepage = "https://github.com/alexandergknoll/nirimap";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "nirimap";
  };
}
