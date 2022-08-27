FROM rust:1.63

COPY . .

RUN cargo install --path .

CMD ["magtag_gateway"]
