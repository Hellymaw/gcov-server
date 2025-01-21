{ pkgs, ... }:

rec {
  env.POSTGRES_USER = "postgres";
  env.POSTGRES_PASSWORD = "password";
  env.POSTGRES_DB = "db";
  env.POSTGRES_HOST = "localhost";
  env.POSTGRES_PORT = 5432;
  env.RUST_LOG="info";

  dotenv.enable = true;

  packages = [ pkgs.git pkgs.docker pkgs.openssl ];

  languages = {
    rust.enable = true;
    javascript = {
      enable = true;
      bun.enable = true;
    };
  };


  processes = {
    gitea.exec = "cd dev/gitea; docker compose up";
  };

  services.postgres = {
    enable = true;
    package = pkgs.postgresql_17;
    listen_addresses = "${env.POSTGRES_HOST}";
    port = env.POSTGRES_PORT;
    initialDatabases = [
      {
        name = "${env.POSTGRES_DB}";
        user = "${env.POSTGRES_USER}";
        pass = "${env.POSTGRES_PASSWORD}";
      }
    ];
    initialScript = "CREATE EXTENSION ltree;";
  };
}
