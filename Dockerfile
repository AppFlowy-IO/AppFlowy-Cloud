FROM alphine
COPY ./target/release/appflowy_server /app/
WORKDIR /app/
CMD ["./appflowy_server"]
