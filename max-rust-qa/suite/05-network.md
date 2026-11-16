# 05 — Сетевая игра (TC-05xx)

Сессия через Horizon (gRPC) + headless C++ (ENet, релей).

| ID | Название | Шаги | Ожидание | Автоматизация |
|---|---|---|---|---|
| TC-0501 | Horizon доступен | docker compose up | healthy, WS 8080, gRPC 8082, UDP 32000–32100 | ✅ SMOKE (healthcheck) |
| TC-0502 | gRPC hello/list/create/join | horizon_probe | протокол v1, список игр, создание, подписанный тикет | ✅ SMOKE; UNIT max_net |
| TC-0503 | Лобби-последовательность | 2 клиента join | 28→44→29→47→30→36/35→32→05→34 у обоих | ✅ SMOKE |
| TC-0504 | Хендшейк TP 00/TP 03 | подключение к headless | версии/тикет/data_hash; отказы по 1/2/3 | ✅ UNIT handshake; SMOKE |
| TC-0505 | data_hash fallback | сервер без PATCHES.RES в game_data | полный хэш → reject → MAX-only → принят, без флагов | ✅ SMOKE (два прогона) |
| TC-0506 | Ретрай после Create game | Create из UI | сессия греется 10–15 с → авто-ретрай → InSession | ✅ UNIT; SMOKE |
| TC-0507 | Партия 2 клиентов | два клиента, 10 ходов | turn_counter синхронен, CRC (45) совпадает, 0 таймаутов барьеров | ✅ SMOKE 2×10 |
| TC-0508 | Приказы через релей | move/attack от клиента | пакет 38/08 доходит до сервера и других клиентов | ✅ SMOKE `live-smoke.sh`/`live-smoke-3.sh` (релей, CRC сходится); в `ci.sh` smoke — отдельная стадия `--smoke`/`--smoke3` |
| TC-0509 | Чат через релей | отправить 18 | сообщение у обоих клиентов | ✅ UNIT; WINDOW (ручная проверка была) |
| TC-0510 | End Turn синхронно | оба клиента End Turn | 06+52+45, ход закрыт у обоих | ✅ SMOKE |
| TC-0511 | Расхождение/десинк | намеренный десинк (если есть хук) | сервер шлёт 51/рестарт (как C++) | ⏭ not planned: хука намеренного десинка нет; обратная проверка — `live-smoke-mixed.sh` (C++ `GameManager_CheckDesync` молчит) |
| TC-0512 | Разрыв соединения | убить сеть клиента | реконнект-политика (retry, статус), без паники; партия продолжается | ✅ UNIT/in-process: `three_clients_survive_disconnect_and_converge_via_state_sync` + `resume_state_sync_*`; ⏭ not planned — живой pause/proxy-сценарий не автоматизирован |
| TC-0513 | Соединение с 3+ клиентами | 3 клиента, min_players=3 | релей всем, барьеры 00/52/54/45 по всем командам, CRC совпадает | ✅ SMOKE `build/scripts/live-smoke-3.sh` (3×10 PASS, CRC `0x34D5`) |
| TC-0514 | Спектатор | клиент без команды | наблюдатель видит партию | ⏭ not planned (blocker): нет `TEAM_TYPE_SPECTATOR`, `Remote_SetupPlayers` требует команду, поздний вход без снапшота (protocol.md §9.2) |
| TC-0515 | Выход из сессии | Esc/Menu из сессии | корректное отключение, сессия жива для остальных | ⏭ not planned: требует живого сетевого сценария; in-process `run_network` завершается по Esc без паник (smoke) |

Итог: 15 TC, ✅ 12, 🟡 0, ⏭ 3 (TC-0511, TC-0514, TC-0515); TC-0512 ✅ in-process, живой разрыв — not planned.
