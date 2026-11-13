/* Copyright (c) 2026 M.A.X. Port Team
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

#include "server.hpp"

#include <SDL3/SDL.h>

#include "crash_reporter.hpp"
#include "game_manager.hpp"
#include "menu.hpp"
#include "remote.hpp"
#include "resource_manager.hpp"
#include "transport.hpp"

/* Dedicated server entry point (MAX_HEADLESS builds only, invoked via --server).
 *
 * The server runs the full deterministic simulation as the host peer and acts as
 * the network arbiter, without a display, audio device, or a team of its own.
 * See docs/dedicated-server-plan.md.
 */
int Server_Main() {
    fprintf(stderr, "SERVER: entry\n");
    fflush(stderr);

    try {
        SDL_setenv_unsafe("SDL_VIDEODRIVER", "dummy", 1);
        SDL_setenv_unsafe("SDL_AUDIODRIVER", "dummy", 1);

        fprintf(stderr, "SERVER: initializing resources\n");
        fflush(stderr);

        ResourceManager_InitResources();

        fprintf(stderr, "SERVER: resources ready\n");
        fflush(stderr);

        if (!Transport_Init()) {
            SDL_Log("Transport_Init() failed, the server cannot accept connections.");
        }

        /* Host a network game without taking part in it. Remote_IsServerMode is read from the server_mode setting
         * during Remote_Init, the lobby then waits for players and starts the game by itself. */
        if (Remote_Lobby(true)) {
            menu_update_resource_levels();
            Remote_SendNetPacket_17();

            GameManager_GameLoop(GameManager_GameState);

            ResourceManager_GetMissionManager()->ResetMission();
        }

        Remote_Deinit();

    } catch (const std::exception& e) {
        fprintf(stderr, "SERVER: unhandled exception: %s\n", e.what());
        fflush(stderr);

        ResourceManager_Exit();

        return EXIT_FAILURE;

    } catch (...) {
        fprintf(stderr, "SERVER: unhandled exception of unknown type\n");
        fflush(stderr);

        ResourceManager_Exit();

        return EXIT_FAILURE;
    }

    CrashReporter_Shutdown();

    return EXIT_SUCCESS;
}
