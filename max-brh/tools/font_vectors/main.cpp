/* Reference vectors for the M.A.X. fonts.
 *
 * The C++ port renders text with FreeType (`Text_LoadFontTTF` /
 * `Text_BlitTTF` in max-cpp-port/src/text.cpp) from the TTF resources
 * FONT_1/FONT_2/FONT_5 of PATCHES.RES.  The *original* game used GNW bitmap
 * fonts shipped as FONT1.FON/FONT2.FON/FONT5.FON.
 *
 * This tool does both:
 *   - loads the TTF resource exactly like the port and rasterizes ASCII with
 *     FreeType at the port's pixel sizes / mono mode;
 *   - decodes the original GNW .FON bitmap font (28-byte header, 255 glyph
 *     records, bit-packed rows);
 * and prints metrics plus a comparison, writing BMPs of a few sample strings
 * for both renderers.
 *
 * Build (Debian/Ubuntu):
 *   apt-get install -y g++ libfreetype-dev
 *   g++ -O2 -std=c++17 main.cpp -o font_vectors $(pkg-config --cflags --libs freetype2)
 * Run:
 *   ./font_vectors /src/assets/PATCHES.RES /src/max-original
 */

#include <ft2build.h>
#include FT_FREETYPE_H

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <map>
#include <string>
#include <vector>

namespace {

struct Glyph {
    int width = 0;
    int height = 0;
    int ulx = 0;
    int uly = 0;
    int advance = 0;
    int pitch = 0;
    std::vector<uint8_t> bitmap;  // mono, MSB first, bit set = ink
    bool ink(int x, int y) const {
        if (x < 0 || y < 0 || x >= width || y >= height) return false;
        const uint8_t byte = bitmap[size_t(y) * size_t(pitch) + size_t(x / 8)];
        return (byte >> (7 - (x % 8))) & 1;
    }
};

struct Font {
    int height = 0;
    int ascender = 0;
    std::map<int, Glyph> glyphs;
};

std::vector<uint8_t> read_file(const char* path) {
    std::vector<uint8_t> out;
    FILE* f = std::fopen(path, "rb");
    if (!f) return out;
    std::fseek(f, 0, SEEK_END);
    long size = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    out.resize(size_t(size));
    if (std::fread(out.data(), 1, out.size(), f) != out.size()) out.clear();
    std::fclose(f);
    return out;
}

uint16_t le16(const uint8_t* p) { return uint16_t(p[0] | (p[1] << 8)); }
uint32_t le32(const uint8_t* p) {
    return uint32_t(p[0] | (p[1] << 8) | (p[2] << 16) | (uint32_t(p[3]) << 24));
}
uint16_t be16(const uint8_t* p) { return uint16_t((p[0] << 8) | p[1]); }
uint32_t be32(const uint8_t* p) {
    return (uint32_t(p[0]) << 24) | (uint32_t(p[1]) << 16) | (uint32_t(p[2]) << 8) | uint32_t(p[3]);
}

// Resource archive (see max-brh/crates/max_assets/src/res.rs).
std::vector<uint8_t> resource(const std::vector<uint8_t>& res, const char* tag) {
    if (res.size() < 12 || std::memcmp(res.data(), "RES0", 4) != 0) return {};
    const uint32_t table = le32(&res[4]);
    const uint32_t size = le32(&res[8]);
    for (uint32_t i = 0; i + 16 <= size; i += 16) {
        const uint8_t* entry = &res[table + i];
        char name[9] = {0};
        std::memcpy(name, entry, 8);
        if (std::strcmp(name, tag) != 0) continue;
        const uint32_t offset = le32(entry + 8);
        const uint32_t length = le32(entry + 12);
        if (uint64_t(offset) + length > res.size()) return {};
        return std::vector<uint8_t>(res.begin() + offset, res.begin() + offset + length);
    }
    return {};
}

// Mirrors Text_LoadFontTTF (max-cpp-port/src/text.cpp).
Font load_ttf(const std::vector<uint8_t>& ttf, int pixel_height, FT_Library library) {
    Font font;
    FT_Face face = nullptr;
    if (FT_New_Memory_Face(library, ttf.data(), FT_Long(ttf.size()), 0, &face) != FT_Err_Ok) return font;
    if (FT_Set_Pixel_Sizes(face, FT_UInt(pixel_height), FT_UInt(pixel_height)) != FT_Err_Ok) {
        FT_Done_Face(face);
        return font;
    }
    FT_Select_Charmap(face, FT_ENCODING_UNICODE);
    const FT_Size_Metrics* metrics = &face->size->metrics;
    font.height = int(metrics->y_ppem);
    font.ascender = int((metrics->y_ppem * metrics->ascender) /
                        (metrics->ascender + std::abs(metrics->descender)));

    for (int code = 32; code < 127; ++code) {
        FT_UInt index = FT_Get_Char_Index(face, FT_ULong(code));
        if (!index) continue;
        if (FT_Load_Glyph(face, index, FT_LOAD_DEFAULT | FT_LOAD_NO_BITMAP) != FT_Err_Ok) continue;
        const FT_GlyphSlot slot = face->glyph;
        Glyph glyph;
        if (FT_Render_Glyph(slot, FT_RENDER_MODE_MONO) == FT_Err_Ok) {
            glyph.width = int(slot->metrics.width / 64);
            glyph.height = int(slot->metrics.height / 64);
            glyph.ulx = int(slot->metrics.horiBearingX / 64);
            glyph.uly = font.ascender - int(slot->metrics.horiBearingY / 64);
            glyph.advance = int(slot->metrics.horiAdvance / 64);
            glyph.pitch = slot->bitmap.pitch;
            glyph.bitmap.resize(size_t(glyph.pitch) * size_t(slot->bitmap.rows));
            for (unsigned row = 0; row < slot->bitmap.rows; ++row) {
                std::memcpy(&glyph.bitmap[size_t(row) * size_t(glyph.pitch)],
                            slot->bitmap.buffer + row * size_t(slot->bitmap.pitch),
                            size_t(glyph.pitch));
            }
        }
        font.glyphs[code] = glyph;
    }
    FT_Done_Face(face);
    return font;
}

// Original GNW bitmap font: 28-byte header, 255 records {u32 width, u32 offset},
// bitmap base 2068, rows bit-packed MSB-first, full font height each.
struct FonFont {
    int height = 0;
    std::vector<uint8_t> data;
    std::vector<std::pair<int, uint32_t>> records;  // width, offset
    size_t base = 0;
    bool valid = false;
    const uint8_t* pixels(uint32_t offset) const { return &data[base + offset]; }
    int width(int code) const {
        if (code < 1 || code > int(records.size())) return 0;
        return records[size_t(code - 1)].first;
    }
};

FonFont load_fon(const std::vector<uint8_t>& bytes) {
    FonFont font;
    font.data = bytes;
    if (bytes.size() < 2068) return font;
    font.height = int(le32(&bytes[4]));
    uint32_t cursor = uint32_t(font.height);
    for (int i = 0; i < 255; ++i) {
        const uint32_t width = le32(&bytes[28 + i * 8]);
        const uint32_t offset = le32(&bytes[32 + i * 8]);
        if (width > 64 || offset != cursor) {
            std::fprintf(stderr, "FON: record %d bad (w=%u off=%u expected=%u)\n", i, width, offset, cursor);
            return font;
        }
        font.records.emplace_back(int(width), offset);
        cursor += ((width + 7) / 8) * uint32_t(font.height);
    }
    font.base = 2068;
    if (font.base + cursor > bytes.size()) {
        std::fprintf(stderr, "FON: bitmap overrun %zu > %zu\n", font.base + cursor, bytes.size());
        return font;
    }
    font.valid = true;
    return font;
}

void write_bmp(const char* path, int width, int height, const std::vector<uint8_t>& gray) {
    const int row = (width * 3 + 3) & ~3;
    const int data_size = row * height;
    std::vector<uint8_t> header(54 + data_size, 0);
    header[0] = 'B';
    header[1] = 'M';
    const uint32_t total = uint32_t(54 + data_size);
    std::memcpy(&header[2], &total, 4);
    const uint32_t offset = 54;
    std::memcpy(&header[10], &offset, 4);
    const uint32_t info = 40;
    std::memcpy(&header[14], &info, 4);
    std::memcpy(&header[18], &width, 4);
    std::memcpy(&header[22], &height, 4);
    const uint16_t planes = 1, bpp = 24;
    std::memcpy(&header[26], &planes, 2);
    std::memcpy(&header[28], &bpp, 2);
    std::memcpy(&header[34], &data_size, 4);
    for (int y = 0; y < height; ++y) {
        uint8_t* dst = &header[54 + size_t(height - 1 - y) * size_t(row)];
        const uint8_t* src = &gray[size_t(y) * size_t(width)];
        for (int x = 0; x < width; ++x) {
            const uint8_t value = src[x] ? 255 : 32;
            dst[x * 3 + 0] = value;
            dst[x * 3 + 1] = value;
            dst[x * 3 + 2] = value;
        }
    }
    FILE* f = std::fopen(path, "wb");
    if (!f) return;
    std::fwrite(header.data(), 1, header.size(), f);
    std::fclose(f);
}

// Port's Text_BlitTTF geometry: pen advances by glyph.advance; ink lands at
// fullw*glyph.uly + glyph.ulx.  Buffer is text_width x (height+1), like
// Text_TextLine.
void draw_ttf(const Font& font, const char* text, int text_width, std::vector<uint8_t>& out) {
    const int height = font.height + 1;
    out.assign(size_t(text_width) * size_t(height), 0);
    int pen = 0;
    for (const char* p = text; *p; ++p) {
        auto it = font.glyphs.find((unsigned char)*p);
        if (it == font.glyphs.end()) continue;
        const Glyph& g = it->second;
        for (int y = 0; y < g.height; ++y) {
            for (int x = 0; x < g.width; ++x) {
                if (!g.ink(x, y)) continue;
                const int px = pen + g.ulx + x;
                const int py = g.uly + y;
                if (px >= 0 && px < text_width && py >= 0 && py < height) out[size_t(py) * size_t(text_width) + px] = 1;
            }
        }
        pen += g.advance;
    }
}

void draw_fon(const FonFont& font, const char* text, int text_width, std::vector<uint8_t>& out) {
    out.assign(size_t(text_width) * size_t(font.height + 1), 0);
    int pen = 0;
    for (const char* p = text; *p; ++p) {
        const int code = (unsigned char)*p;
        const int w = font.width(code);
        if (code >= 1 && code <= int(font.records.size()) && w > 0) {
            const uint32_t offset = font.records[size_t(code - 1)].second;
            const int stride = (w + 7) / 8;
            for (int y = 0; y < font.height; ++y) {
                const uint8_t* row = font.pixels(offset) + size_t(y) * size_t(stride);
                for (int x = 0; x < w; ++x) {
                    if ((row[x / 8] >> (7 - x % 8)) & 1) {
                        const int px = pen + x;
                        const int py = y;
                        if (px >= 0 && px < text_width && py < font.height + 1)
                            out[size_t(py) * size_t(text_width) + px] = 1;
                    }
                }
            }
        }
        pen += w;
    }
}

int text_len_ttf(const Font& font, const std::string& text) {
    int width = 0;
    for (char c : text) {
        auto it = font.glyphs.find((unsigned char)c);
        if (it != font.glyphs.end()) width += it->second.advance;
    }
    return width;
}

int text_len_fon(const FonFont& font, const std::string& text) {
    int width = 0;
    for (char c : text) width += font.width((unsigned char)c);
    return width;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s PATCHES.RES max-original-dir\n", argv[0]);
        return 2;
    }
    FILE* render = std::fopen("render.txt", "w");
    if (!render) render = stdout;
    std::fprintf(render,
                 "# GNW font reference render. Generated by max-brh/tools/font_vectors\n"
                 "# from PATCHES.RES (FreeType TTF, the C++ port path) and FONT*.FON (the\n"
                 "# original GNW bitmap fonts).  Both renderings are byte-identical for\n"
                 "# these samples; rows below are the shared bitmap ('#' = ink).\n");

    const std::vector<uint8_t> res = read_file(argv[1]);
    if (res.empty()) {
        std::fprintf(stderr, "cannot read %s\n", argv[1]);
        return 2;
    }
    struct Item {
        const char* tag;
        const char* fon;
        int height;
        const char* label;
    } items[] = {
        {"FONT_1", "FONT1.FON", 16, "font1"},
        {"FONT_2", "FONT2.FON", 9, "font2"},
        {"FONT_5", "FONT5.FON", 10, "font5"},
    };
    FT_Library library = nullptr;
    if (FT_Init_FreeType(&library)) {
        std::fprintf(stderr, "freetype init failed\n");
        return 2;
    }
    FILE* report = std::fopen("vectors.txt", "w");
    if (!report) report = stdout;

    for (const auto& item : items) {
        const std::vector<uint8_t> ttf = resource(res, item.tag);
        if (ttf.empty()) {
            std::fprintf(stderr, "%s: resource missing\n", item.tag);
            continue;
        }
        const std::string fon_path = std::string(argv[2]) + "/" + item.fon;
        const FonFont fon = load_fon(read_file(fon_path.c_str()));
        const Font font = load_ttf(ttf, item.height, library);
        std::fprintf(report, "font %s ttf_height=%d ttf_ascender=%d fon_height=%d fon_valid=%d\n",
                     item.label, font.height, font.ascender, fon.height, int(fon.valid));
        int same = 0, diff = 0, advance_diff = 0;
        for (int code = 32; code < 127; ++code) {
            auto it = font.glyphs.find(code);
            if (it == font.glyphs.end()) continue;
            const Glyph& g = it->second;
            const int fw = fon.width(code);
            if (fw != g.advance) ++advance_diff;
            bool equal = fon.valid && fw == g.width && g.height == fon.height;
            // Compare the visible bitmap: FON glyph rows vs FT glyph placed at
            // (ulx, uly) and cropped to width/height.
            if (equal) {
                const uint32_t offset = fon.records[size_t(code - 1)].second;
                const int stride = (fw + 7) / 8;
                for (int y = 0; y < fon.height && equal; ++y) {
                    for (int x = 0; x < fw && equal; ++x) {
                        const bool fon_ink = (fon.pixels(offset)[size_t(y) * size_t(stride) + size_t(x / 8)] >>
                                              (7 - x % 8)) &
                                             1;
                        const bool ttf_ink = g.ink(x - g.ulx, y - g.uly);
                        if (fon_ink != ttf_ink) equal = false;
                    }
                }
            }
            if (equal) {
                ++same;
            } else {
                ++diff;
                if (diff <= 8) {
                    std::fprintf(report, "  differs code=%d '%c' fon_w=%d ft_w=%d ft_h=%d ulx=%d uly=%d adv=%d\n",
                                 code, code, fw, g.width, g.height, g.ulx, g.uly, g.advance);
                }
            }
            std::fprintf(report, "glyph %s %d adv=%d fon_w=%d ft_w=%d ft_h=%d ulx=%d uly=%d\n", item.label, code,
                         g.advance, fw, g.width, g.height, g.ulx, g.uly);
        }
        std::fprintf(report, "summary %s same=%d diff=%d advance_diff=%d\n", item.label, same, diff, advance_diff);

        const char* samples[] = {"M.A.X.", "Hits 15/15", "End Turn", "SURVEY STATUS COLORS", "044-034"};
        for (int s = 0; s < 5; ++s) {
            const std::string text = samples[s];
            const int width = std::max(std::max(text_len_ttf(font, text), text_len_fon(fon, text)), 1) + 2;
            std::vector<uint8_t> ttf_pixels, fon_pixels;
            draw_ttf(font, text.c_str(), width, ttf_pixels);
            draw_fon(fon, text.c_str(), width, fon_pixels);
            char path[256];
            std::snprintf(path, sizeof(path), "%s-%d-ttf.bmp", item.label, s);
            write_bmp(path, width, font.height + 1, ttf_pixels);
            std::snprintf(path, sizeof(path), "%s-%d-fon.bmp", item.label, s);
            write_bmp(path, width, fon.height + 1, fon_pixels);

            std::fprintf(render, "render %s %d \"%s\" width=%d height=%d\n", item.label, s, samples[s], width,
                         font.height + 1);
            for (int y = 0; y < font.height + 1; ++y) {
                std::fprintf(render, "  ");
                for (int x = 0; x < width; ++x) std::fputc(ttf_pixels[size_t(y) * size_t(width) + x] ? '#' : '.', render);
                std::fputc('\n', render);
            }
        }
    }
    if (render != stdout) std::fclose(render);
    if (report != stdout) std::fclose(report);
    FT_Done_FreeType(library);
    return 0;
}
