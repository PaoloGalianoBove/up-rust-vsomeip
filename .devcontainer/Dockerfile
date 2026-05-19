FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# 1. Installa tutti i tool di sistema
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
        curl \
        vim && \
    rm -rf /var/lib/apt/lists/*

# 2. Crea utente ubuntu con sudo senza password (se non c’è già)
ARG USERNAME=ubuntu
ARG USER_UID=1000
ARG USER_GID=1000
RUN if ! id -u $USERNAME > /dev/null 2>&1; then \
        groupadd --gid $USER_GID $USERNAME && \
        useradd --uid $USER_UID --gid $USER_GID -m $USERNAME && \
        echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/$USERNAME && \
        chmod 0440 /etc/sudoers.d/$USERNAME; \
    fi

# 3. Passa all’utente developer per rustup
USER $USERNAME
WORKDIR /home/$USERNAME

# 4. Installa Rust (stable e nightly) per l’utente di sviluppo, aggiorna PATH
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y && \
    $HOME/.cargo/bin/rustup toolchain install stable && \
    $HOME/.cargo/bin/rustup default stable && \
    $HOME/.cargo/bin/rustup toolchain install nightly

ENV PATH="/home/$USERNAME/.cargo/bin:${PATH}"

# Opzionale: metti il path anche in .bashrc
RUN echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> $HOME/.bashrc

# 5. Torna root (se vuoi, altrimenti lascia user dev!)
USER root

# 6. Permesso sudo generale senza password (potresti già averlo con sopra)
RUN echo "ALL ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/nopw && chmod 0440 /etc/sudoers.d/nopw

# 7. Aggiungi VSCode extensions / settings (opzionale, di solito via devcontainer.json!)