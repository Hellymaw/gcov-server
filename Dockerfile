FROM rust:1.79

WORKDIR /usr/src/gcov_server
COPY . .

RUN cargo install --path .

CMD [ "gcov-server" ]