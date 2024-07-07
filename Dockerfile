###### Builder Image
FROM rust:alpine as builder

RUN apk update \
    && apk add musl-dev


# add musl target for musl build
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY . .

# build install binary
RUN cargo install --target=x86_64-unknown-linux-musl --path .

RUN cargo clean

##################
#  Output image  #
##################

FROM alpine:latest

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN addgroup -S $APP_USER \
    && adduser -S -g $APP_USER $APP_USER

RUN apk update \
    && apk add --no-cache ca-certificates tzdata \
    && rm -rf /var/cache/apk/*

WORKDIR /home/${APP_USER}

COPY --from=builder /usr/local/cargo/bin/cachyos_discord_bot /usr/bin/

RUN chown -R $APP_USER:$APP_USER .

USER $APP_USER

CMD ["/usr/bin/cachyos_discord_bot"]
