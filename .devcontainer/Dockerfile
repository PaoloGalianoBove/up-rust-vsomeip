FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y \
        sudo \
        libboost-all-dev \
        cmake \
        libssl-dev \
        pkg-config \
        libclang-dev \
        build-essential \
        clang \
        g++ \
        git \
        cargo \
        curl \
        vim && \
    rm -rf /var/lib/apt/lists/*

# Eventuale: dai a tutti gli utenti permesso sudo senza password
RUN echo "ALL ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/nopw \
    && chmod 0440 /etc/sudoers.d/nopw

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y && \
    /root/.cargo/bin/rustup toolchain install stable && \
    /root/.cargo/bin/rustup default stable
ENV PATH="/root/.cargo/bin:${PATH}"