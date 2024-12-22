{ pkgs, lib, config, inputs, ... }:

rec {
  env.POSTGRES_PASSWORD = "password";
  env.POSTGRES_DB = "db";

  packages = [ pkgs.git ];

  languages.rust.enable = true;

  services.postgres = {
    enable = true;
    listen_addresses = "127.0.0.1";
    initialDatabases = [
      {
        name = "${env.POSTGRES_DB}";
        user = "postgres";
        pass = "${env.POSTGRES_PASSWORD}";
      }
    ];
  };
}
