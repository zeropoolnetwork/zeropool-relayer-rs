# General purpose Dockerfile for indexer and relayer services



FROM rust:latest as build
ARG PACKAGE
ARG BINARY_MASK
ARG FEATURES=""

ENV PACKAGE=$PACKAGE
ENV BINARY_MASK=$BINARY_MASK
ENV FEATURES=$FEATURES

RUN echo "Building $PACKAGE with features $FEATURES (binary mask: $BINARY_MASK))"

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
RUN rm ./target/release/deps/$BINARY_MASK*
COPY ./$PACKAGE/src ./src
RUN cargo build --release --features "$FEATURES"

# Final image
FROM rust:latest
ARG PACKAGE

COPY --from=build /zeropool-$PACKAGE/target/release/zeropool-$PACKAGE .
ENV BINARY_NAME=zeropool-$PACKAGE
CMD /$BINARY_NAME
