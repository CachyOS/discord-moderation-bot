###### Builder Image
FROM rust:latest as builder

RUN apt update
RUN apt-get install musl-tools -y

RUN rustup target add x86_64-unknown-linux-musl

# Build the dependencies in a separate step to avoid rebuilding all of them
# every time the source code changes. This takes advantage of Docker's layer
# caching, and it works by copying the Cargo.{toml,lock} with dummy source code
# and doing a full build with it.
WORKDIR /app
COPY Cargo.lock Cargo.toml build.rs /app/
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs

RUN RUSTFLAGS=-Clinker=musl-gcc cargo build --release --target=x86_64-unknown-linux-musl

# Dependencies are now cached, copy the actual source code and do another full
# build. The touch on all the .rs files is needed, otherwise cargo assumes the
# source code didn't change thanks to mtime weirdness.
RUN rm -rf /app/src
COPY . /app/
RUN find src -name "*.rs" -exec touch {} \; && \
RUSTFLAGS=-Clinker=musl-gcc cargo build --release --target=x86_64-unknown-linux-musl

##################
#  Output image  #
##################

FROM alpine:latest

ENV TZ=Etc/UTC
ENV APP_USER=appuser

RUN addgroup -g 1000 $APP_USER \
 && adduser -D -s /bin/sh -u 1000 -G $APP_USER $APP_USER

WORKDIR /home/${APP}/bin

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/cachyos_discord_bot .

RUN chown -R $APP_USER:$APP_USER .

USER $APP_USER

CMD ["./cachyos_discord_bot"]
