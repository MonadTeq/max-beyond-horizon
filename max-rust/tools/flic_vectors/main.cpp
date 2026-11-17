// Reference-vector generator for the FLC decoder (`max_assets::flic`).
//
// The canon is the C++ port: `max-cpp-port/src/flicsmgr.cpp`. This program is a
// self-contained copy of the frame loader and of the chunk decoders used by
// `flicsmgr_read` / `flicsmgr_decode_frame`, without SDL and without the game:
//
//   * `Flicsmgr_load_frame`       — 16-byte frame header + chunk bytes
//   * `flicsmgr_decode_frame`     — chunk dispatch
//   * `flicsmgr_decode_color`     — palette chunks (shifted into the 6-bit palette)
//   * `flicsmgr_decode_byte_run`  — chunk 15 (BYTE_RUN)
//   * `flicsmgr_decode_delta_flc` — chunk 7  (DELTA_FLC / FLI_SS2)
//
// The buffer is a plain `width × height` byte array with `full = width` — exactly
// the 128×128 scratch window that `GameManager_MenuCreateFlic` hands to
// `flicsmgr_construct(base_unit.GetFlicsAnimation(), &GameManager_Flic.sw, …)`.
//
// Build & run (Debian bookworm; the rust:1.94.0-bookworm image has g++):
//   g++ -O2 -std=c++17 -Wall -Wextra -Werror main.cpp -o gen
//   ./gen ../../../max-original > vectors.txt
//
// Output format v1 ('#' lines are comments):
//   vector <file> <frame> <width> <height> <hex pixels, width*height*2 chars>
//   digests <file> <frame-count> <hash-0> <hash-1> …   (hash = u32 hex)
//   files <count> <frame-total>                        (summary, last line)
//
// `hash` is a wrapping FNV-1a over the decoded frame bytes — trivial to
// reproduce in Rust and sensitive to any pixel difference.

#include <algorithm>
#include <cctype>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <string>
#include <vector>

namespace {

constexpr uint16_t FLI_TYPE = 0xAF11u;
constexpr uint16_t FLC_TYPE = 0xAF12u;
constexpr uint16_t FRAME_TYPE = 0xF1FAu;

enum ChunkTypes {
    COLOR_256 = 4,
    DELTA_FLC = 7,
    COLOR_64 = 11,
    BLACK = 13,
    BYTE_RUN = 15,
    LITERAL = 16
};

// ---------------------------------------------------------------------------
// Little-endian helpers (no unaligned pointer casts: the code must be defined
// behaviour even though the original walks raw pointers).
// ---------------------------------------------------------------------------

uint16_t read_u16(const uint8_t* p) {
    return static_cast<uint16_t>(p[0] | (static_cast<uint16_t>(p[1]) << 8));
}

int16_t read_i16(const uint8_t* p) { return static_cast<int16_t>(read_u16(p)); }

uint32_t read_u32(const uint8_t* p) {
    return static_cast<uint32_t>(p[0]) | (static_cast<uint32_t>(p[1]) << 8) |
           (static_cast<uint32_t>(p[2]) << 16) | (static_cast<uint32_t>(p[3]) << 24);
}

void write_u16(uint8_t* p, uint16_t value) {
    p[0] = static_cast<uint8_t>(value & 0xFF);
    p[1] = static_cast<uint8_t>(value >> 8);
}

// ---------------------------------------------------------------------------
// Flic state — the subset of `struct Flic` used by the decoders.
// ---------------------------------------------------------------------------

struct Palette {
    uint8_t entries[256][3]{};
};

struct Flic {
    std::vector<uint8_t> buffer;
    int32_t width{};
    int32_t height{};
    int32_t full{};
    Palette palette;
};

struct Frame {
    uint32_t size{};
    uint16_t type{};
    uint16_t chunks{};
    std::vector<uint8_t> buffer;
};

struct Header {
    uint32_t size{};
    uint16_t type{};
    uint16_t frames{};
    uint16_t width{};
    uint16_t height{};
    uint16_t depth{};
    uint16_t flags{};
    uint32_t speed{};
    uint32_t oframe1{};
    uint32_t oframe2{};
};

bool read_header(std::FILE* fp, Header* header) {
    std::vector<uint8_t> raw(128, 0);

    if (std::fread(raw.data(), 1, raw.size(), fp) != raw.size()) {
        return false;
    }

    header->size = read_u32(&raw[0]);
    header->type = read_u16(&raw[4]);
    header->frames = read_u16(&raw[6]);
    header->width = read_u16(&raw[8]);
    header->height = read_u16(&raw[10]);
    header->depth = read_u16(&raw[12]);
    header->flags = read_u16(&raw[14]);
    header->speed = read_u32(&raw[16]);
    header->oframe1 = read_u32(&raw[80]);
    header->oframe2 = read_u32(&raw[84]);

    return header->type == FLC_TYPE;
}

bool load_frame(std::FILE* fp, Frame* frame) {
    uint8_t raw[16];

    if (std::fread(raw, 1, sizeof(raw), fp) != sizeof(raw)) {
        return false;
    }

    const int32_t size_field = static_cast<int32_t>(read_u32(&raw[0]));

    frame->type = read_u16(&raw[4]);
    frame->chunks = read_u16(&raw[6]);
    frame->size = static_cast<uint32_t>(size_field - 16);
    frame->buffer.assign(frame->size, 0);

    if (frame->type != FRAME_TYPE) {
        return false;
    }

    return std::fread(frame->buffer.data(), 1, frame->size, fp) == frame->size;
}

// `flicsmgr_decode_color`: palette chunks. The portrait never loads the FLIC
// palette (`load_flic_palette = 0`), but the decoder still walks the packets.
void decode_color(const uint8_t* buffer, int32_t shift, Palette* palette) {
    int32_t entry = 0;
    const uint8_t* cbuf = buffer;
    int16_t ops = read_i16(cbuf);

    cbuf += 2;

    while (--ops >= 0) {
        entry += *cbuf++;

        int32_t count = *cbuf++;

        if (count == 0) {
            count = 256;
        }

        while (--count >= 0) {
            palette->entries[entry][0] = static_cast<uint8_t>(cbuf[0] >> shift);
            palette->entries[entry][1] = static_cast<uint8_t>(cbuf[1] >> shift);
            palette->entries[entry][2] = static_cast<uint8_t>(cbuf[2] >> shift);

            cbuf += 3;
            entry++;
        }
    }
}

// `flicsmgr_decode_byte_run`: every line starts with a packet-count byte that
// the original simply skips, then runs of `int8_t count` + byte / literals.
void decode_byte_run(const uint8_t* buffer, Flic* flc) {
    int32_t height = flc->height;
    const uint8_t* cpt = buffer;
    int32_t y = 0;

    while (--height >= 0) {
        int32_t x = 0;
        int8_t psize = 0;
        int32_t offset = y;

        cpt++;

        while ((x += psize) < flc->width) {
            psize = static_cast<int8_t>(*cpt++);

            if (psize >= 0) {
                std::memset(&flc->buffer[static_cast<size_t>(offset)], *cpt++,
                            static_cast<size_t>(psize));
            } else {
                psize = static_cast<int8_t>(-psize);
                std::memcpy(&flc->buffer[static_cast<size_t>(offset)], cpt,
                            static_cast<size_t>(psize));
                cpt += psize;
            }

            offset += psize;
        }

        y += flc->full;
    }
}

// `flicsmgr_decode_delta_flc`: word-oriented delta (FLI_SS2).
void decode_delta_flc(const uint8_t* buffer, Flic* flc) {
    const uint8_t* cursor = buffer;
    int16_t line_count = read_i16(cursor);

    cursor += 2;

    uint8_t* line_address = flc->buffer.data();

    while (--line_count >= 0) {
        int16_t opt_word = read_i16(cursor);

        cursor += 2;

        while (static_cast<uint16_t>(opt_word) & 0x8000u) {
            if (static_cast<uint16_t>(opt_word) & 0x4000u) {
                opt_word = static_cast<int16_t>(-opt_word);
                line_address += opt_word * flc->full;
            } else {
                line_address[flc->width - 1] = static_cast<uint8_t>(opt_word);
            }

            opt_word = read_i16(cursor);
            cursor += 2;
        }

        int16_t packet_count = opt_word;
        int32_t offset = 0;

        while (--packet_count >= 0) {
            const uint8_t column_skip_count = cursor[0];
            int8_t packet_type = static_cast<int8_t>(cursor[1]);

            cursor += 2;
            offset += column_skip_count;

            if (packet_type >= 0) {
                std::memcpy(&line_address[offset], cursor,
                            static_cast<size_t>(packet_type) * 2u);
                cursor += packet_type * 2;
                offset += packet_type * 2;
            } else {
                const uint16_t word = read_u16(cursor);

                cursor += 2;
                packet_type = static_cast<int8_t>(-packet_type);

                while (--packet_type >= 0) {
                    write_u16(&line_address[offset], word);
                    offset += 2;
                }
            }
        }

        line_address += flc->full;
    }
}

uint32_t chunk_header_size(const uint8_t* chunk) { return read_u32(chunk); }

uint16_t chunk_header_type(const uint8_t* chunk) { return read_u16(chunk + 4); }

// `flicsmgr_decode_frame`
void decode_frame(const Frame& frame, Flic* flc) {
    const uint8_t* next_chunk = frame.buffer.data();

    for (int32_t i = 0; i < frame.chunks; i++) {
        const uint32_t size = chunk_header_size(next_chunk);
        const uint16_t type = chunk_header_type(next_chunk);
        const uint8_t* body = next_chunk + 6;

        next_chunk += size;

        switch (type) {
            case COLOR_256:
                decode_color(body, 2, &flc->palette);
                break;
            case DELTA_FLC:
                decode_delta_flc(body, flc);
                break;
            case COLOR_64:
                decode_color(body, 0, &flc->palette);
                break;
            case BLACK:
                std::memset(flc->buffer.data(), 0, flc->buffer.size());
                break;
            case BYTE_RUN:
                decode_byte_run(body, flc);
                break;
            case LITERAL:
                std::memcpy(flc->buffer.data(), body,
                            static_cast<size_t>(flc->width) * static_cast<size_t>(flc->height));
                break;
            default:
                break;
        }
    }
}

uint32_t hash_frame(const uint8_t* pixels, size_t count) {
    uint32_t hash = 2166136261u;

    for (size_t i = 0; i < count; i++) {
        hash ^= pixels[i];
        hash *= 16777619u;
    }

    return hash;
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

struct Vector {
    std::string file;
    int frame;
};

const std::vector<Vector>& dump_list() {
    static const std::vector<Vector> list = {
        {"AGT.FLC", 0},        // BRUN + COLOR + PSTAMP: полный первый кадр
        {"AGT.FLC", 1},        // первый DELTA_FLC поверх предыдущего кадра
        {"AGT.FLC", 29},       // последний кадр (29 дельт подряд)
        {"AA_ISO.FLC", 0},     // одиночный кадр (35 ресурсов MAX.RES)
        {"COMMTWR.FLC", 0},    // одиночный кадр здания
        {"SAVELOAD.FLC", 0},   // 128×256, другой размер поля
    };

    return list;
}

bool decode_file(const std::filesystem::path& path, std::FILE* out) {
    std::FILE* fp = std::fopen(path.string().c_str(), "rb");

    if (!fp) {
        std::fprintf(stderr, "cannot open %s\n", path.string().c_str());
        return false;
    }

    Header header{};

    if (!read_header(fp, &header)) {
        std::fprintf(stderr, "bad FLIC header: %s\n", path.string().c_str());
        std::fclose(fp);
        return false;
    }

    if (std::fseek(fp, header.oframe1, SEEK_SET) != 0) {
        std::fprintf(stderr, "seek failed: %s\n", path.string().c_str());
        std::fclose(fp);
        return false;
    }

    Flic flc;
    flc.width = header.width;
    flc.height = header.height;
    flc.full = header.width;
    flc.buffer.assign(static_cast<size_t>(flc.width) * static_cast<size_t>(flc.height), 0);

    const std::string name = path.filename().string();
    std::vector<uint32_t> digests;

    for (int index = 0; index < header.frames; index++) {
        Frame frame;

        if (!load_frame(fp, &frame)) {
            std::fprintf(stderr, "frame %d load failed: %s\n", index, name.c_str());
            std::fclose(fp);
            return false;
        }

        decode_frame(frame, &flc);
        digests.push_back(hash_frame(flc.buffer.data(), flc.buffer.size()));

        for (const auto& vector : dump_list()) {
            if (vector.file != name || vector.frame != index) {
                continue;
            }

            std::fprintf(out, "vector %s %d %d %d ", name.c_str(), index, flc.width, flc.height);

            for (uint8_t value : flc.buffer) {
                std::fprintf(out, "%02x", value);
            }

            std::fprintf(out, "\n");
        }
    }

    std::fclose(fp);

    std::fprintf(out, "digests %s %d", name.c_str(), header.frames);

    for (uint32_t value : digests) {
        std::fprintf(out, " %08x", value);
    }

    std::fprintf(out, "\n");

    return true;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: %s <flc-directory> [file ...]\n", argv[0]);
        return 2;
    }

    const std::filesystem::path directory = argv[1];
    std::vector<std::filesystem::path> files;

    if (argc > 2) {
        for (int i = 2; i < argc; i++) {
            files.emplace_back(directory / argv[i]);
        }
    } else {
        for (const auto& entry : std::filesystem::directory_iterator(directory)) {
            std::string name = entry.path().filename().string();
            std::string lower = name;

            std::transform(lower.begin(), lower.end(), lower.begin(),
                           [](unsigned char c) { return static_cast<char>(std::tolower(c)); });

            if (entry.is_regular_file() && lower.size() > 4 && lower.substr(lower.size() - 4) == ".flc") {
                files.push_back(entry.path());
            }
        }

        std::sort(files.begin(), files.end());
    }

    std::fprintf(stdout,
                 "# flic_vectors v1 — C++ эталон (max-cpp-port/src/flicsmgr.cpp).\n"
                 "# vector <file> <frame> <width> <height> <hex pixels>\n"
                 "# digests <file> <frames> <fnv1a-32 каждого кадра>\n"
                 "# files <count> <frames>\n");

    int decoded = 0;
    int frames = 0;

    for (const auto& path : files) {
        std::FILE* probe = std::fopen(path.string().c_str(), "rb");

        if (!probe) {
            continue;
        }

        Header header{};

        if (read_header(probe, &header)) {
            frames += header.frames;
        }

        std::fclose(probe);

        if (!decode_file(path, stdout)) {
            return 1;
        }

        decoded++;
    }

    std::fprintf(stdout, "files %d %d\n", decoded, frames);

    return decoded > 0 ? 0 : 1;
}
