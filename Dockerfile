# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake clang libclang-dev libtesseract-dev libleptonica-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim

ARG OCR_LANGUAGES="eng spa jpn ron"
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libtesseract5 libgomp1 python3-venv \
    fonts-noto-core fonts-noto-extra fonts-noto-cjk \
    $(printf 'tesseract-ocr-%s ' ${OCR_LANGUAGES}) \
    && rm -rf /var/lib/apt/lists/*

ENV PATH="/opt/venv/bin:${PATH}"
RUN python3 -m venv /opt/venv \
    && pip install --no-cache-dir "torch==2.8.0+cpu" \
       --extra-index-url https://download.pytorch.org/whl/cpu \
    && pip install --no-cache-dir ctranslate2 huggingface_hub sentencepiece sacremoses "transformers<5"

COPY --from=build /app/target/release/afterprint /usr/local/bin/afterprint
RUN afterprint --help >/dev/null && ct2-transformers-converter --help >/dev/null

WORKDIR /data
ENTRYPOINT ["afterprint"]
CMD ["--help"]
