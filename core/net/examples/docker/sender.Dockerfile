FROM rust:1.40

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --examples --bins
EXPOSE 9000

CMD ./core/net/examples/docker/sender.sh