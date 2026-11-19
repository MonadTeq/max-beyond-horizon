#!/usr/bin/env bash
# Очистка Docker после CI/сборочных прогонов.
#   bash build/scripts/docker-clean.sh          # безопасно: только мусор
#   bash build/scripts/docker-clean.sh --all    # агрессивно: все неиспользуемые образы
set -e

ALL=0
if [ "${1:-}" = "--all" ]; then ALL=1; fi

echo "== docker-clean: остановленные контейнеры =="
docker ps -aq -f status=exited | xargs -r docker rm -v

if [ "$ALL" = "1" ]; then
    echo "== docker-clean: ВСЕ остановленные контейнеры =="
    docker ps -aq | xargs -r docker rm -v
fi

echo "== docker-clean: dangling-образы (без тегов) =="
docker image prune -f

if [ "$ALL" = "1" ]; then
    echo "== docker-clean: все неиспользуемые образы (не только dangling) =="
    docker image prune -a -f
fi

echo "== docker-clean: build-cache =="
docker builder prune -f

echo "== docker-clean: итог =="
docker system df
