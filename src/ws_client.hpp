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

#ifndef WS_CLIENT_HPP
#define WS_CLIENT_HPP

#include <cstdint>
#include <string>

/* Minimal RFC 6455 WebSocket client for querying the Horizon lobby (max_lobby:list_games).
 * Plain ws:// only (no TLS), blocking, single request/response per call. The client frames are
 * masked as required by the standard; the server frames are expected unmasked. */

/* Performs a full WebSocket exchange:
 *  - TCP connect to host:port
 *  - opening handshake
 *  - sends the text payload (e.g. {"event":"max_lobby:list_games"})
 *  - reads frames until a text frame arrives or the timeout expires
 * Returns the text payload of the first text frame, or an empty string on any failure.
 */
std::string WsQueryJson(const std::string& host, uint16_t port, const std::string& payload,
                        uint32_t timeout_ms);

#endif /* WS_CLIENT_HPP */
