# Архитектура проекта

Клиент-серверная архитектура Rust-клиента M.A.X. под Horizon, протоколы и модели.

| Документ | Содержимое |
|---|---|
| [plan-modes.md](plan-modes.md) | Три режима игры (одиночная, hot-seat, сеть) и принцип визуального паритета 1:1 |
| [protocol.md](protocol.md) | Полный контракт сети: WS/gRPC Horizon + ENet-сессия, ~70 сообщений побайтово |
| [protocol-grpc.md](protocol-grpc.md) | protobuf-схемы Horizon (horizon.v1), решения, JSON↔proto |
| [state-model.md](state-model.md) | Модель состояния партии (15 перечислений, 11 структур, проекция снапшота) |
| [barrier-flow.md](barrier-flow.md) | Lockstep-барьеры 00/52/45/54 и релей в звезде |
| [src-fork.md](src-fork.md) | Отношение src/ (наша развилка) к max-cpp-port (оракулу) |
| [dedicated-server-plan.md](dedicated-server-plan.md) | Эксплуатация C++-headless под Horizon |

## Общая схема

```
Rust-клиент (max-brh)              Horizon (max-horizon-server)        C++ headless (src/)
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
- **src/** — наша развилка C++-порта: headless-сервер-арбитр (релей пакетов, CRC-барьеры).
- **max-horizon-server** — оркестратор сессий (Rust).

## Референсы-оракулы (не изменяются)

- `max-cpp-port/` — канонический код M.A.X. Port (klei1984/max, git-субмодуль).
- `max-cpp-port-max-brh-docs/` — документация автора (save-формат V70, Watcom-конструкции, сеть).
- `max-original/` — оригинальная игра; `max-original-dasm/` — наши RE-артефакты.
