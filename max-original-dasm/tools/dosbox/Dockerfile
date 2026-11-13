# Рантайм для динамического анализа MAXRUN.EXE под DOSBox-X.
# Сборка: docker build -t max-dosbox -f Dockerfile .
FROM ubuntu:24.04

RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
        python3 libasound2t64 libsdl2-2.0-0 libsdl2-net-2.0-0 libpng16-16t64 zlib1g \
        libslirp0 libgl1 libx11-6 libxrandr2 libxkbfile1 libncurses6 libtinfo6 libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src/max-original
ENTRYPOINT ["/bin/bash", "-c"]
