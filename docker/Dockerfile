# General purpose Dockerfile for indexer and relayer services

FROM rust:latest as build
ARG PACKAGE
ARG FEATURES=""
ENV PACKAGE=$PACKAGE
ENV FEATURES=$FEATURES

RUN apt-get update && apt-get install -y clang

# Cache dependencies
COPY ./indexer-tx-storage /indexer-tx-storage
RUN USER=root cargo new --bin zeropool-$PACKAGE
WORKDIR /zeropool-"$PACKAGE"
COPY ./Cargo.lock ./Cargo.lock
COPY ./$PACKAGE/Cargo.toml ./Cargo.toml
RUN cargo build --release --features "$FEATURES"

# Build
RUN rm src/*.rs
ENV BINARY_NAME=zeropool-$PACKAGE
RUN /bin/bash -c 'rm ./target/release/deps/"${BINARY_NAME//-/_}"*'
COPY ./$PACKAGE/src ./src
RUN cargo build --release --features "$FEATURES"

# Final image
FROM rust:latest
ARG PACKAGE
ENV BINARY_NAME=zeropool-$PACKAGE

WORKDIR /app
COPY --from=build /zeropool-$PACKAGE/target/release/$BINARY_NAME .
CMD /app/$BINARY_NAME
