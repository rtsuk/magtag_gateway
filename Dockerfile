FROM rust:1.65

COPY . .

RUN cargo install --path .

CMD ["magtag_gateway"]
