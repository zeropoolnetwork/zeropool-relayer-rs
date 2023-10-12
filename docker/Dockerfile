FROM rust:1.73 as build

ARG FEATURES=""
ENV FEATURES=$FEATURES

RUN apt-get update && apt-get install -y clang

# Cache dependencies
RUN USER=root cargo new --bin zeropool-relayer
WORKDIR /zeropool-relayer
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release --features "$FEATURES"

# Build
RUN rm src/*.rs
RUN /bin/bash -c 'rm ./target/release/deps/zeropool_relayer*'
COPY ./src ./src
RUN cargo build --release --features "$FEATURES"

# Final image
FROM rust:1.73

WORKDIR /app
COPY --from=build /zeropool-relayer/target/release/zeropool-relayer /app/zeropool-relayer
CMD /app/zeropool-relayer
