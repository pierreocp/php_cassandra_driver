FROM php:8.3-cli-bookworm AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        cmake \
        curl \
        libclang-dev \
        make \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup update stable

WORKDIR /app
COPY Cargo.toml ./
COPY .cargo ./.cargo
COPY src ./src
RUN cargo build --release

FROM php:8.3-cli-bookworm AS runtime

COPY --from=build /app/target/release/libphp_cassandra_driver.so /tmp/php_cassandra_driver.so
RUN extension_dir="$(php-config --extension-dir)" \
    && cp /tmp/php_cassandra_driver.so "${extension_dir}/php_cassandra_driver.so" \
    && echo 'extension=php_cassandra_driver.so' > /usr/local/etc/php/conf.d/php_cassandra_driver.ini \
    && rm /tmp/php_cassandra_driver.so

WORKDIR /app
COPY examples ./examples
COPY README.md ./README.md

CMD ["php", "examples/test.php"]

FROM scratch AS artifact
COPY --from=build /app/target/release/libphp_cassandra_driver.so /php_cassandra_driver.so
