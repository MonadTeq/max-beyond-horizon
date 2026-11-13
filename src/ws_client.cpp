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

#include "ws_client.hpp"

#include <SDL3/SDL_log.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#else
#include <sys/socket.h>
#include <sys/select.h>

#include <netdb.h>

#include <cerrno>
#include <fcntl.h>
#include <unistd.h>
#endif

#include <array>
#include <chrono>
#include <cstring>
#include <random>
#include <thread>

namespace {

#ifdef _WIN32
using SocketHandle = SOCKET;
constexpr SocketHandle InvalidSocket = INVALID_SOCKET;
constexpr int SocketWouldBlock = WSAEWOULDBLOCK;

inline int SocketClose(SocketHandle handle) { return closesocket(handle); }
inline int SocketErrorCode() { return WSAGetLastError(); }

inline bool SocketSetNonBlocking(SocketHandle handle, bool enabled) {
    u_long mode = enabled ? 1 : 0;
    return ioctlsocket(handle, FIONBIO, &mode) == 0;
}

struct SocketStartupData {
    WSADATA wsa_data{};
};

inline bool SocketStartup(SocketStartupData& data) { return WSAStartup(MAKEWORD(2, 2), &data.wsa_data) == 0; }
inline void SocketCleanup() { WSACleanup(); }
#else
using SocketHandle = int;
constexpr SocketHandle InvalidSocket = -1;
constexpr int SocketWouldBlock = EINPROGRESS;

inline int SocketClose(SocketHandle handle) { return close(handle); }
inline int SocketErrorCode() { return errno; }

inline bool SocketSetNonBlocking(SocketHandle handle, bool enabled) {
    const int flags = fcntl(handle, F_GETFL, 0);

    if (flags < 0) {
        return false;
    }

    return fcntl(handle, F_SETFL, enabled ? (flags | O_NONBLOCK) : (flags & ~O_NONBLOCK)) == 0;
}

struct SocketStartupData {};

inline bool SocketStartup(SocketStartupData&) { return true; }
inline void SocketCleanup() {}
#endif

struct WsSocket {
    SocketHandle handle{InvalidSocket};

    WsSocket() = default;
    WsSocket(const WsSocket&) = delete;
    WsSocket& operator=(const WsSocket&) = delete;

    ~WsSocket() {
        if (handle != InvalidSocket) {
            SocketClose(handle);
        }
    }
};

std::string Base64(const uint8_t* data, size_t size) {
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string out;

    out.reserve((size + 2) / 3 * 4);

    for (size_t i = 0; i < size; i += 3) {
        const uint32_t a = data[i];
        const uint32_t b = (i + 1 < size) ? data[i + 1] : 0;
        const uint32_t c = (i + 2 < size) ? data[i + 2] : 0;
        const uint32_t triple = (a << 16) | (b << 8) | c;

        out.push_back(alphabet[(triple >> 18) & 0x3F]);
        out.push_back(alphabet[(triple >> 12) & 0x3F]);
        out.push_back((i + 1 < size) ? alphabet[(triple >> 6) & 0x3F] : '=');
        out.push_back((i + 2 < size) ? alphabet[triple & 0x3F] : '=');
    }

    return out;
}

/* A masked client frame: FIN | opcode=1 (text), mask bit set. */
std::string EncodeTextFrame(const std::string& text, uint32_t mask_key) {
    std::string frame;

    frame.push_back(static_cast<char>(0x81));

    if (text.size() < 126) {
        frame.push_back(static_cast<char>(0x80 | text.size()));

    } else if (text.size() < 65536) {
        frame.push_back(static_cast<char>(0x80 | 126));
        frame.push_back(static_cast<char>((text.size() >> 8) & 0xFF));
        frame.push_back(static_cast<char>(text.size() & 0xFF));

    } else {
        frame.push_back(static_cast<char>(0x80 | 127));
        for (int32_t shift = 56; shift >= 0; shift -= 8) {
            frame.push_back(static_cast<char>((static_cast<uint64_t>(text.size()) >> shift) & 0xFF));
        }
    }

    for (int32_t i = 3; i >= 0; --i) {
        frame.push_back(static_cast<char>((mask_key >> (i * 8)) & 0xFF));
    }

    for (size_t i = 0; i < text.size(); ++i) {
        frame.push_back(static_cast<char>(text[i] ^ ((mask_key >> ((3 - (i % 4)) * 8)) & 0xFF)));
    }

    return frame;
}

bool SendAll(SocketHandle socket, const char* data, int32_t size) {
    int32_t sent = 0;

    while (sent < size) {
        const int32_t result = send(socket, data + sent, size - sent, 0);

        if (result <= 0) {
            return false;
        }

        sent += result;
    }

    return true;
}

/* Reads until the payload length is known; returns false on error/EOF. */
bool ReadFrameHeader(SocketHandle socket, uint8_t& opcode, uint64_t& payload_size, bool& masked,
                     std::array<uint8_t, 4>& mask_key, uint32_t timeout_ms) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);

    const auto wait_readable = [&]() -> bool {
        while (true) {
            fd_set read_set;
            FD_ZERO(&read_set);
            FD_SET(socket, &read_set);

            timeval tv{};
            const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
                                       deadline - std::chrono::steady_clock::now())
                                       .count();

            if (remaining <= 0) {
                return false;
            }

            tv.tv_sec = static_cast<long>(remaining / 1000);
            tv.tv_usec = static_cast<long>((remaining % 1000) * 1000);

            const int32_t result = select(0, &read_set, nullptr, nullptr, &tv);

            if (result > 0) {
                return true;
            }

            if (result < 0) {
                return false;
            }
        }
    };

    uint8_t header[2]{};

    if (!wait_readable() || recv(socket, reinterpret_cast<char*>(header), 2, 0) != 2) {
        return false;
    }

    opcode = header[0] & 0x0F;
    masked = (header[1] & 0x80) != 0;
    payload_size = header[1] & 0x7F;

    if (payload_size == 126) {
        uint8_t ext[2]{};

        if (!wait_readable() || recv(socket, reinterpret_cast<char*>(ext), 2, 0) != 2) {
            return false;
        }

        payload_size = (static_cast<uint64_t>(ext[0]) << 8) | ext[1];

    } else if (payload_size == 127) {
        uint8_t ext[8]{};

        if (!wait_readable() || recv(socket, reinterpret_cast<char*>(ext), 8, 0) != 8) {
            return false;
        }

        payload_size = 0;

        for (int32_t i = 0; i < 8; ++i) {
            payload_size = (payload_size << 8) | ext[i];
        }
    }

    if (masked) {
        if (!wait_readable() || recv(socket, reinterpret_cast<char*>(mask_key.data()), 4, 0) != 4) {
            return false;
        }
    }

    return true;
}

} // namespace

std::string WsQueryJson(const std::string& host, uint16_t port, const std::string& payload,
                        uint32_t timeout_ms) {
    SocketStartupData startup_data;

    if (!SocketStartup(startup_data)) {
        SDL_Log("WS: socket startup failed (errno %d).", SocketErrorCode());
        return {};
    }

    addrinfo hints{};
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;

    addrinfo* address_list = nullptr;

    const std::string port_string = std::to_string(port);

    if (getaddrinfo(host.c_str(), port_string.c_str(), &hints, &address_list) != 0 || !address_list) {
        SDL_Log("WS: getaddrinfo failed for %s:%s (errno %d).", host.c_str(), port_string.c_str(), SocketErrorCode());
        SocketCleanup();
        return {};
    }

    WsSocket socket;

    for (addrinfo* address = address_list; address; address = address->ai_next) {
        socket.handle = ::socket(address->ai_family, address->ai_socktype, address->ai_protocol);

        if (socket.handle == InvalidSocket) {
            SDL_Log("WS: socket() failed (errno %d).", SocketErrorCode());
            continue;
        }

        /* Non-blocking connect with a bounded wait. */
        SocketSetNonBlocking(socket.handle, true);

        const int32_t connect_result = connect(socket.handle, address->ai_addr, static_cast<int32_t>(address->ai_addrlen));

        if (connect_result < 0 && SocketErrorCode() != SocketWouldBlock) {
            SDL_Log("WS: connect() failed (errno %d).", SocketErrorCode());
            continue;
        }

        fd_set write_set;
        FD_ZERO(&write_set);
        FD_SET(socket.handle, &write_set);

        timeval tv{};
        tv.tv_sec = 0;
        tv.tv_usec = 500000;

        const int32_t select_result = select(static_cast<int>(socket.handle) + 1, nullptr, &write_set, nullptr, &tv);

        if (select_result <= 0) {
            SDL_Log("WS: connect select() failed (result %d, errno %d).", select_result, SocketErrorCode());
            continue;
        }

        SocketSetNonBlocking(socket.handle, false);

        break;
    }

    freeaddrinfo(address_list);

    if (socket.handle == InvalidSocket) {
        SDL_Log("WS: could not connect to %s:%u.", host.c_str(), port);
        SocketCleanup();
        return {};
    }

    std::mt19937 rng(static_cast<uint32_t>(std::chrono::steady_clock::now().time_since_epoch().count()));
    uint8_t key_bytes[16]{};

    for (uint8_t& byte : key_bytes) {
        byte = static_cast<uint8_t>(rng());
    }

    const std::string handshake = "GET / HTTP/1.1\r\n"
                                  "Host: " + host + ":" + port_string + "\r\n"
                                  "Upgrade: websocket\r\n"
                                  "Connection: Upgrade\r\n"
                                  "Sec-WebSocket-Key: " + Base64(key_bytes, sizeof(key_bytes)) + "\r\n"
                                  "Sec-WebSocket-Version: 13\r\n"
                                  "\r\n";

    if (!SendAll(socket.handle, handshake.data(), static_cast<int32_t>(handshake.size()))) {
        SDL_Log("WS: handshake send failed (errno %d).", SocketErrorCode());
        SocketCleanup();
        return {};
    }

    /* Read the handshake response (headers end with \r\n\r\n). */
    std::string response;
    char buffer[512]{};

    while (response.find("\r\n\r\n") == std::string::npos && response.size() < 16384) {
        const int32_t received = recv(socket.handle, buffer, sizeof(buffer), 0);

        if (received <= 0) {
            SDL_Log("WS: handshake receive failed (received %d, errno %d).", received, SocketErrorCode());
            SocketCleanup();
            return {};
        }

        response.append(buffer, received);
    }

    if (response.find(" 101 ") == std::string::npos) {
        SDL_Log("WS handshake rejected: %s", response.substr(0, response.find("\r\n")).c_str());
        SocketCleanup();
        return {};
    }

    const uint32_t mask_key = static_cast<uint32_t>(rng());
    const std::string frame = EncodeTextFrame(payload, mask_key);

    if (!SendAll(socket.handle, frame.data(), static_cast<int32_t>(frame.size()))) {
        SocketCleanup();
        return {};
    }

    /* Collect the first text frame (skipping ping/pong/continuations). */
    std::string result;
    bool received_text = false;

    while (!received_text) {
        uint8_t opcode{};
        uint64_t payload_size{};
        bool masked{};
        std::array<uint8_t, 4> mask_key_received{};

        if (!ReadFrameHeader(socket.handle, opcode, payload_size, masked, mask_key_received, timeout_ms)) {
            SDL_Log("WS: no frame received within %u ms.", timeout_ms);
            break;
        }

        std::string payload_data;
        payload_data.reserve(static_cast<size_t>(std::min<uint64_t>(payload_size, 1 << 20)));

        while (payload_size > 0) {
            const int32_t chunk = static_cast<int32_t>(std::min<uint64_t>(payload_size, sizeof(buffer)));
            const int32_t received = recv(socket.handle, buffer, chunk, 0);

            if (received <= 0) {
                break;
            }

            if (masked) {
                for (int32_t i = 0; i < received; ++i) {
                    buffer[i] ^= static_cast<char>(mask_key_received[(payload_data.size() + i) % 4]);
                }
            }

            payload_data.append(buffer, received);
            payload_size -= received;
        }

        if (opcode == 0x01) {
            result = std::move(payload_data);
            received_text = true;

        } else if (opcode == 0x08 || opcode == 0x00) {
            break;
        }
    }

    SocketCleanup();

    return result;
}
