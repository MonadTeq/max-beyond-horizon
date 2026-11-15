# M.A.X. Port

![Build Workflow](https://github.com/klei1984/max/workflows/Build%20Workflow/badge.svg)
[![Discord](https://img.shields.io/discord/1085572457466974248?label=discord&logo=discord&logoColor=ffffff)](https://discord.gg/TCn8DpeBaY)
[![YouTube Channel Views](https://img.shields.io/youtube/channel/views/UCDnKVYEpcgZZz8okGIC4vkA)](https://www.youtube.com/channel/UCDnKVYEpcgZZz8okGIC4vkA)
![Downloads](https://img.shields.io/github/downloads/klei1984/max/total)

![max](https://user-images.githubusercontent.com/53688147/218547807-5738f04f-e64f-4d59-9790-b3e248faed9b.jpg)

M.A.X. Port is an [SDL library](https://wiki.libsdl.org/) based runtime executable for the 1996 DOS game M.A.X.: Mechanized Assault & Exploration developed and published by Interplay Productions. Portions of this software are copyright © 2023 The FreeType Project (www.freetype.org). All rights reserved.

Official home page: [https://klei1984.github.io/max/](https://klei1984.github.io/max/)

The goal of the project is to fix the game breaking [defects](https://klei1984.github.io/max/defects/) present in the original game executable and to create a runtime that natively runs on modern operating systems.

M.A.X. Port requires the original game data. The port only provides a modernized executable.

M.A.X. COPYRIGHT (C) 1996 INTERPLAY PRODUCTIONS.  ALL RIGHTS RESERVED.
INTERPLAY PRODUCTIONS IS THE EXCLUSIVE LICENSEE AND DISTRIBUTOR.

### How to play M.A.X. Port

Visit the [Installation Guideline](https://klei1984.github.io/max/install/) article found on the home page.

### Rust-клиент, dedicated server и CI

- `max-rust/` — Rust-клиент M.A.X. (gRPC → ENet → лобби → игра).
- `docker-compose.yml` — Horizon + C++-headless одной командой
  (`docker compose up --build`), порты 8080/tcp, 8082/tcp, 32000-32100/udp.
  Инструкция: [max-horizon-server/README.Docker.md](max-horizon-server/README.Docker.md).
- `bash ci.sh` — локальный CI (M6.3): тесты `max-rust` и `max-horizon-server` в
  Docker; `bash ci.sh --smoke` добавляет живой smoke на 2 клиента. GitHub-версия —
  [.github/workflows/ci.yml](.github/workflows/ci.yml).
- Реконнект (M6.1-заготовка): `--reconnect-attempts` / `--reconnect-timeout` в
  `max_app`; восстановление транспорта ENet+`TP 00`, полное восстановление партии
  — M6.1-финал (`docs/protocol.md` §9).
