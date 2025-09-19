FROM rust:1.80 as builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN useradd -ms /bin/bash appuser
COPY --from=builder /app/target/release/rustiscorm-runtime /usr/local/bin/rustiscorm-runtime
COPY migrations ./migrations
ENV RUST_LOG=info
USER appuser
CMD ["/usr/local/bin/rustiscorm-runtime"]

