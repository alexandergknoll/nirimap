# Home Manager module for nirimap.
#
# Takes the flake's `self` so the default package comes from the same revision
# as the module.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.nirimap;
  tomlFormat = pkgs.formats.toml { };
in
{
  options.programs.nirimap = {
    enable = lib.mkEnableOption "nirimap, a minimap overlay for the Niri Wayland compositor";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.nirimap;
      defaultText = lib.literalExpression "nirimap.packages.\${system}.nirimap";
      description = "The nirimap package to install.";
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      example = lib.literalExpression ''
        {
          display = {
            anchor = "top-right";
            workspace_mode = "all";
          };
          appearance.focused_color = "#89b4fa";
          behavior.always_visible = true;
        }
      '';
      description = ''
        Settings written to {file}`$XDG_CONFIG_HOME/nirimap/config.toml`. See
        the project README for the available options.

        Left empty, the file is not managed and nirimap writes its own
        commented default config on first run.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."nirimap/config.toml" = lib.mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "nirimap-config.toml" cfg.settings;
    };
  };
}
