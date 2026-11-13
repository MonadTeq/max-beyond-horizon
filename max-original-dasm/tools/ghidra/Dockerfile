# Ghidra headless для реверса MAXRUN.EXE (DOS/4GW LE).
FROM eclipse-temurin:21-jdk

RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
        wget unzip fontconfig libfreetype6 libxrender1 libxtst6 libxi6 \
    && rm -rf /var/lib/apt/lists/*

RUN wget -q https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_11.3.2_build/ghidra_11.3.2_PUBLIC_20250415.zip \
        -O /tmp/ghidra.zip \
    && unzip -q /tmp/ghidra.zip -d /opt \
    && rm /tmp/ghidra.zip \
    && ln -s /opt/ghidra_11.3.2_PUBLIC /opt/ghidra

ENV GHIDRA=/opt/ghidra
WORKDIR /work
