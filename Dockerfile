FROM ubuntu:22.04

# Installiere notwendige Abhängigkeiten inklusive C-Compiler
RUN apt-get update && \
    apt-get install -y curl build-essential gcc-mingw-w64-x86-64

# Installiere Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Füge Windows-Target hinzu
RUN rustup target add x86_64-pc-windows-gnu

WORKDIR /app
COPY . .

# Setze Linker-Umgebungsvariablen
ENV CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
ENV CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
ENV AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar

# Führe den Build aus
RUN cargo build --release --target x86_64-pc-windows-gnu
