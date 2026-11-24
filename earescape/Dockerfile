FROM rust:slim-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ear-escape /app/ear-escape
COPY data /app/data
COPY static /app/static
COPY migrations /app/migrations
ENV PORT=10000
EXPOSE 10000
CMD ["/app/ear-escape"]
