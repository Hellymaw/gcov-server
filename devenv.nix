{ pkgs, lib, config, inputs, ... }:

rec {
  env.POSTGRES_USER = "postgres";
  env.POSTGRES_PASSWORD = "password";
  env.POSTGRES_DB = "db";
  env.POSTGRES_HOST = "localhost";
  env.POSTGRES_PORT = 5432;

  packages = [ pkgs.git ];

  languages.rust.enable = true;

  services.postgres = {
    enable = true;
    listen_addresses = "${env.POSTGRES_HOST}";
    port = env.POSTGRES_PORT;
    initialDatabases = [
      {
        name = "${env.POSTGRES_DB}";
        user = "${env.POSTGRES_USER}";
        pass = "${env.POSTGRES_PASSWORD}";
      }
    ];
  };
}
