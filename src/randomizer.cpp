/* Copyright (c) 2025 M.A.X. Port Team
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

#include "randomizer.hpp"

#include <SDL3/SDL_log.h>
#include <XoshiroCpp.hpp>
#include <cstdio>
#include <ctime>
#include <memory>

static std::unique_ptr<XoshiroCpp::Xoshiro128StarStar> Randomizer_Object;
static uint64_t Randomizer_DrawCount;
static uint64_t Randomizer_DrawHash;

void Randomizer_Init() {
    Randomizer_Object = std::make_unique<XoshiroCpp::Xoshiro128StarStar>(XoshiroCpp::SplitMix64(std::time(nullptr))());
}

uint32_t Randomizer_GetNext() {
    uint32_t value = (*Randomizer_Object)();

    ++Randomizer_DrawCount;
    Randomizer_DrawHash = (Randomizer_DrawHash ^ value) * 0x00000100000001B3ull;

    return value;
}

void Randomizer_SetSeed(const uint32_t seed) {
    Randomizer_Object = std::make_unique<XoshiroCpp::Xoshiro128StarStar>(XoshiroCpp::SplitMix64(seed)());
    Randomizer_DrawCount = 0;
    Randomizer_DrawHash = 0;

    SDL_Log("DBG rng cp seed draws=%llu hash=%016llX", static_cast<unsigned long long>(Randomizer_DrawCount),
            static_cast<unsigned long long>(Randomizer_DrawHash));
}

void Randomizer_Checkpoint(const char* site) {
    SDL_Log("DBG rng cp %s draws=%llu hash=%016llX", site, static_cast<unsigned long long>(Randomizer_DrawCount),
            static_cast<unsigned long long>(Randomizer_DrawHash));
}
