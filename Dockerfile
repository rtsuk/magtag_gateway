FROM rust:1.69

COPY . .

RUN cargo install --path .

CMD ["magtag_gateway"]
