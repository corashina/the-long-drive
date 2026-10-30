FROM rust@sha256:6ff07edce8775d0f64be7aba9197229407301bddf2054d62c27b541a6238a181
RUN rustup target add wasm32-unknown-unknown && rustup component add rustfmt \
    && cargo install wasm-bindgen-cli --version 0.2.129 --locked --root /opt/wasm-bindgen
RUN curl -fsSL -o /tmp/binaryen.tar.gz https://github.com/WebAssembly/binaryen/releases/download/version_133/binaryen-version_133-x86_64-linux.tar.gz \
    && echo '2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489  /tmp/binaryen.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/binaryen.tar.gz -C /opt && mv /opt/binaryen-version_133 /opt/binaryen && rm /tmp/binaryen.tar.gz
ENV PATH="/opt/wasm-bindgen/bin:/opt/binaryen/bin:${PATH}"
ENV CARGO_HOME=/cargo
ENV CARGO_TARGET_DIR=/target
WORKDIR /workspace
