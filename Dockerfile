FROM oven/bun:1.3.1 AS frontend-builder

WORKDIR /app/front

COPY front/package.json front/bun.lock ./
RUN bun install --frozen-lockfile

COPY front/ ./
RUN bun run build

FROM rust:1.91.0 AS backend-builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY --from=frontend-builder /app/front/dist ./front/dist

RUN cargo build --release

FROM debian:13-slim

WORKDIR /app

COPY --from=backend-builder /app/target/release/chat_sacha_house ./chat.sacha.house

RUN chmod +x ./chat.sacha.house
RUN mkdir -p /app/db

ENV BIND_HOST=0.0.0.0

EXPOSE 3030

ENTRYPOINT ["./chat.sacha.house"]