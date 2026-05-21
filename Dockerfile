# Build stage
FROM rust:1.87 AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Runtime stage
FROM gcr.io/distroless/cc-debian12

COPY --from=builder /app/target/release/telegram_ollama_bot /telegram_ollama_bot

CMD ["/telegram_ollama_bot"]
