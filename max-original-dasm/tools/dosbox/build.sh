# Сборка DOSBox-X с встроенным дебаггером (dynamic analysis MAXRUN.EXE).
# Запуск: docker run --rm -v "D:/dev/max-port-debug:/src" -w /src ubuntu:24.04 \
#          bash /src/max-original-dasm/tools/dosbox/build.sh
set -euxo pipefail

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq build-essential autoconf automake libtool pkg-config nasm \
    libsdl2-dev libsdl2-net-dev libpng-dev zlib1g-dev libslirp-dev \
    libasound2-dev libx11-dev libxkbfile-dev libgl1-mesa-dev libncurses-dev >/dev/null

cd /src/max-original-dasm/tools/dosbox-x
# Репозиторий выгружен на Windows с CRLF — чиним шебанги и autotools-шаблоны.
find . -type f \( -name '*.ac' -o -name '*.am' -o -name '*.in' -o -name '*.m4' \
    -o -name '*.sh' -o -name '*.pl' -o -name '*.h' -o -name '*.c' \
    -o -name '*.cpp' -o -name '*.hpp' \) -exec sed -i 's/\r$//' {} +
./autogen.sh
./configure --enable-debug=heavy --enable-sdl2 --disable-avcodec
make -j"$(nproc)"

ls -la src/dosbox-x
