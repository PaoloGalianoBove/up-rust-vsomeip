FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# 1. Installa i tool necessari
RUN apt-get update && \
    apt-get install -y \
        sudo \
        iproute2 \
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
        wget \
        vim \
        net-tools \
        iputils-ping \
        findutils \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# 2. Crea o configura l'utente con sudo senza password
ARG USERNAME=ubuntu
ARG USER_UID=1000
ARG USER_GID=1000
RUN if id -u $USERNAME > /dev/null 2>&1; then \
        usermod -u $USER_UID $USERNAME && \
        groupmod -g $USER_GID $USERNAME; \
    else \
        groupadd --gid $USER_GID $USERNAME && \
        useradd --uid $USER_UID --gid $USER_GID -m $USERNAME; \
    fi && \
    mkdir -p /home/$USERNAME && \
    chown -R $USER_UID:$USER_GID /home/$USERNAME && \
    echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/$USERNAME && \
    chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME
WORKDIR /home/$USERNAME

# 3. Installa Rust per l’utente
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y && \
    $HOME/.cargo/bin/rustup default stable

ENV PATH="/home/$USERNAME/.cargo/bin:${PATH}"

# 4. Copia i sorgenti con l'owner corretto (ignora target/ per Docker!)
COPY --chown=$USERNAME:$USERNAME ./light-switch /home/$USERNAME/light-switch
COPY --chown=$USERNAME:$USERNAME ./up-transport-vsomeip-rust /home/$USERNAME/up-transport-vsomeip-rust

WORKDIR /home/$USERNAME/light-switch

# 5. Rileva e imposta la versione stdlib C++ corretta
RUN CPP_STD_VER=$(ls /usr/include/c++/ | grep -E '^[0-9]+$' | head -1) && \
    echo "export GENERIC_CPP_STDLIB_PATH=/usr/include/c++/$CPP_STD_VER" >> $HOME/.bashrc && \
    echo "export ARCH_SPECIFIC_CPP_STDLIB_PATH=/usr/include/x86_64-linux-gnu/c++/$CPP_STD_VER" >> $HOME/.bashrc
RUN rm -rf /home/$USERNAME/light-switch/target*
ARG CPP_STD_VER=13
ENV GENERIC_CPP_STDLIB_PATH="/usr/include/c++/${CPP_STD_VER}"
ENV ARCH_SPECIFIC_CPP_STDLIB_PATH="/usr/include/x86_64-linux-gnu/c++/${CPP_STD_VER}"
RUN cargo build

# 6. Lavora direttamente nella cartella del progetto
WORKDIR /home/$USERNAME/light-switch

# 9. Esponi porte vsomeip
EXPOSE 30491 30492 30490/udp

# 10. Fai partire una bash session come utente non-root
CMD [ "/bin/bash" ]
