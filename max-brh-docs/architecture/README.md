# Архитектура проекта

Архитектура клиента M.A.X. и выделенного сервера Horizon, протоколы и модели.
Документация разделена на два подкаталога:

## Архитектура клиента — [`client/`](client/)

Игра для одного игрока, hot-seat и сетевая игра в режиме Peer-to-Peer через
TCP/IP (один из игроков — хост).

| Документ | Содержимое |
|---|---|
| [plan-modes.md](client/plan-modes.md) | Три режима игры (одиночная, hot-seat, сеть) и принцип визуального паритета 1:1 |
| [game-mechanics.md](client/game-mechanics.md) | Игровые механики (ресурсы, производство, бой, движение) — контракт для сверки |
| [state-model.md](client/state-model.md) | Модель состояния партии (15 перечислений, 11 структур, проекция снапшота) |
| [barrier-flow.md](client/barrier-flow.md) | Lockstep-барьеры 00/52/45/54 и релей в звезде |
| [protocol.md](client/protocol.md) | Сетевой контракт: ENet-сессия + WS к Horizon, ~70 сообщений побайтово |

## Архитектура выделенного сервера — [`server/`](server/)

Игроки подключаются к выделенному серверу через лобби.

| Документ | Содержимое |
|---|---|
| [dedicated-server-plan.md](server/dedicated-server-plan.md) | План выделенного сервера: C++-headless под Horizon |
| [lobby-ui.md](server/lobby-ui.md) | UI-лобби сервера (игровой лобби + админ-консоль) — отдельно от игры |
| [protocol-grpc.md](server/protocol-grpc.md) | protobuf-схемы Horizon (horizon.v1), решения, JSON↔proto |

## Общая схема

```
Rust-клиент (max-brh)              Horizon (max-brh-server)        C++ headless (max-cpp-port)
┌─────────────────────┐   gRPC      ┌──────────────────────┐  spawn     ┌──────────────────┐
│ max_app (UI/цикл)   │◄───────────►│ WS+gRPC лобби/админ   │───────────►│ max-port --server │
│ max_render (визуал) │   ENet/UDP  │ тикеты, сессии        │            │ релей/арбитр     │
│ max_core (симуляция)│◄───────────►│                       │            │ барьеров         │
│ max_net (протокол)  │             └──────────────────────┘            └──────────────────┘
│ max_assets (форматы)│
└─────────────────────┘
```

- **max_core** — полная детерминированная lockstep-симуляция (бит-в-бит с C++), чистые функции без I/O.
- **max_net** — бинарные протоколы: gRPC (Horizon) + UDP/ENet-семантика (сессия).
- **max_assets/max_render** — форматы оригинальных ресурсов и визуал 1:1.
- **max-cpp-port** — оракул C++-порта; из него собирается headless-сервер-арбитр (`--server`: релей пакетов, CRC-барьеры).
- **max-brh-server** — оркестратор сессий (Rust).

## Референсы-оракулы (не изменяются)

- `max-cpp-port/` — канонический код M.A.X. Port (klei1984/max, git-субмодуль).
- `max-cpp-port-docs/` — документация автора (save-формат V70, Watcom-конструкции, сеть).
- `max-original/` — оригинальная игра; `max-original-dasm/` — RE-артефакты проекта.
