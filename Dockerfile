FROM rust:latest as builder

ENV USER=root
ENV SQLX_OFFLINE=true
ENV DATABASE_URL=sqlite:database/database.sqlite

# Build the dependencies in a separate step to avoid rebuilding all of them
# every time the source code changes. This takes advantage of Docker's layer
# caching, and it works by copying the Cargo.{toml,lock} with dummy source code
# and doing a full build with it.
WORKDIR /cachyos_discord_bot
COPY Cargo.lock Cargo.toml build.rs /cachyos_discord_bot/
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs

RUN cargo fetch
RUN cargo build --release

# Dependencies are now cached, copy the actual source code and do another full
# build. The touch on all the .rs files is needed, otherwise cargo assumes the
# source code didn't change thanks to mtime weirdness.
RUN rm -rf /cachyos_discord_bot/src
COPY . /cachyos_discord_bot/
RUN find src -name "*.rs" -exec touch {} \; && cargo build --release


##################
#  Output image  #
##################

FROM debian:bullseye-slim

ARG APP=/usr/src/app

ENV TZ=Etc/UTC
ENV APP_USER=appuser

WORKDIR ${APP}

RUN apt-get update \
 && apt-get install -y ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd $APP_USER \
 && useradd -g $APP_USER $APP_USER

COPY --from=builder /cachyos_discord_bot/target/release/cachyos_discord_bot .

RUN mkdir database
RUN chown -R $APP_USER:$APP_USER .

USER $APP_USER

CMD ["./cachyos_discord_bot"]
