// The head of a layered network, written to the instruction set.
//
// This is the part that cannot be made incremental: the accumulator carries
// over from move to move, but the head runs whole at every evaluation, and at
// sixteen dot products of twelve hundred and eighty it dwarfs everything else
// in the evaluation. Plain Rust gets it to about two microseconds; the node
// costs three. So it is worth writing by hand.
//
// Three paths, chosen once at load time by asking the processor what it has:
//
//   AVX-512 VNNI   `dpbusd` accumulates u8 x i8 straight into i32, sixty-four
//                  lanes at a time, and cannot overflow on the way.
//   AVX2+FMA       `maddubs` pairs into i16 first, thirty-two lanes. That
//                  intermediate CAN saturate -- 255 x 127 x 2 is 64 770 and an
//                  i16 holds 32 767 -- and this accepts it, as the engines
//                  this shape came from accept it: both extremes have to land
//                  on the same pair of neurons, and the measurement against
//                  the exact version says it does not happen in practice.
//   escalar        for anything older, and as the thing the other two are
//                  checked against.
//
// The arithmetic is fixed by how the file was quantised and none of it is a
// free choice. In particular the two squares are taken in opposite orders --
// before the clip in the dual activation, after it at the output -- because
// that is what the trainer does, and swapping them gives a network that loads,
// runs, and evaluates wrong.

#include <cstdint>
#include <cstring>
#include <algorithm>

#if defined(__x86_64__) || defined(_M_X64)
#include <immintrin.h>
#define TEM_X86 1
#endif

namespace {

// The pairwise product of the two halves of one perspective, as bytes.
//
// `mulhrs` on a value already shifted left by seven is `a * b / 256` rounded to
// nearest rather than truncated. Writing it as a plain shift loses half a unit
// per neuron and the error does not cancel.
// `desl` e' o pre-deslocamento, e nao e' uma constante: o `mulhrs` desloca 15,
// e o que se quer e' `(a*b) >> k` com k escolhido para o resultado caber num
// byte. Com QA=255 o produto tem 16 bits e k=8, logo desl=7. Com QA=1024 tem
// 20 bits e k=12, logo desl=3 -- e escrever 7 ali dava `1024 << 7`, que nao
// cabe num i16 e produzia lixo sem rebentar.
inline uint8_t pares_escalar(int16_t a, int16_t b, int qa, int desl) {
    int32_t x = (int32_t)std::min<int32_t>(std::max<int32_t>(a, 0), qa) << desl;
    int32_t y = (int32_t)std::min<int32_t>(std::max<int32_t>(b, 0), qa);
    int32_t r = (((x * y) >> 14) + 1) >> 1;
    return (uint8_t)std::min<int32_t>(std::max<int32_t>(r, 0), 255);
}

struct Forma {
    int largura;   // o acumulador
    int l1;        // saidas da primeira camada da cabeca
    int l2;        // saidas da segunda
    int qa;
    int desl;
    float div;     // de soma inteira para unidades reais
};

// ---------------------------------------------------------------- escalar ---

float cabeca_escalar(const Forma& f, const int16_t* nos, const int16_t* eles,
                     const int8_t* l1w, const float* l1b, const float* l2w,
                     const float* l2b, const float* sw, float sb,
                     uint8_t* buf, float* h, float* g) {
    const int meia = f.largura / 2;
    for (int i = 0; i < meia; i++) {
        buf[i] = pares_escalar(nos[i], nos[i + meia], f.qa, f.desl);
        buf[i + meia] = pares_escalar(eles[i], eles[i + meia], f.qa, f.desl);
    }
    for (int i = 0; i < f.l1; i++) {
        const int8_t* w = l1w + (size_t)i * f.largura;
        int32_t s = 0;
        for (int j = 0; j < f.largura; j++) s += (int32_t)buf[j] * (int32_t)w[j];
        float v = (float)s * f.div + l1b[i];
        h[i] = std::min(std::max(v, 0.0f), 1.0f);
        h[i + f.l1] = std::min(std::max(v * v, 0.0f), 1.0f);
    }
    for (int i = 0; i < f.l2; i++) g[i] = l2b[i];
    for (int j = 0; j < f.l1 * 2; j++) {
        const float* w = l2w + (size_t)j * f.l2;
        float v = h[j];
        for (int i = 0; i < f.l2; i++) g[i] += v * w[i];
    }
    float s = sb;
    for (int i = 0; i < f.l2; i++) {
        float v = std::min(std::max(g[i], 0.0f), 1.0f);
        s += v * v * sw[i];
    }
    return s;
}

#ifdef TEM_X86

// ------------------------------------------------------------------- AVX2 ---

__attribute__((target("avx2")))
void pares_avx2(const int16_t* p, uint8_t* saida, int meia, int qa, int desl) {
    const __m256i zero = _mm256_setzero_si256();
    const __m256i tecto = _mm256_set1_epi16((short)qa);
    // `slli` quer uma constante; `mullo` por 2^desl faz o mesmo com um valor.
    const __m256i fator = _mm256_set1_epi16((short)(1 << desl));
    for (int i = 0; i < meia; i += 16) {
        __m256i a = _mm256_loadu_si256((const __m256i*)(p + i));
        __m256i b = _mm256_loadu_si256((const __m256i*)(p + i + meia));
        a = _mm256_min_epi16(_mm256_max_epi16(a, zero), tecto);
        b = _mm256_min_epi16(_mm256_max_epi16(b, zero), tecto);
        // `slli` por 7 e depois `mulhrs`: (a*b)/256 com arredondamento.
        __m256i r = _mm256_mulhrs_epi16(_mm256_mullo_epi16(a, fator), b);
        // Empacotar para bytes SEM baralhar a ordem: `packus` trabalha por
        // metade de registo, e sem esta permutacao os neuronios trocavam de
        // sitio -- um erro que nao rebenta, so' avalia mal.
        __m256i lo = _mm256_permute2x128_si256(r, r, 0x00);
        __m256i hi = _mm256_permute2x128_si256(r, r, 0x11);
        __m256i pk = _mm256_packus_epi16(lo, hi);
        _mm_storeu_si128((__m128i*)(saida + i), _mm256_castsi256_si128(pk));
    }
}

__attribute__((target("avx2")))
int32_t escalar_u8i8_avx2(const uint8_t* a, const int8_t* w, int n) {
    __m256i soma = _mm256_setzero_si256();
    const __m256i uns = _mm256_set1_epi16(1);
    for (int j = 0; j < n; j += 32) {
        __m256i x = _mm256_loadu_si256((const __m256i*)(a + j));
        __m256i y = _mm256_loadu_si256((const __m256i*)(w + j));
        __m256i p = _mm256_maddubs_epi16(x, y);
        soma = _mm256_add_epi32(soma, _mm256_madd_epi16(p, uns));
    }
    __m128i s = _mm_add_epi32(_mm256_castsi256_si128(soma),
                              _mm256_extracti128_si256(soma, 1));
    s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0x4e));
    s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0xb1));
    return _mm_cvtsi128_si32(s);
}

__attribute__((target("avx2,fma")))
float cabeca_avx2(const Forma& f, const int16_t* nos, const int16_t* eles,
                  const int8_t* l1w, const float* l1b, const float* l2w,
                  const float* l2b, const float* sw, float sb,
                  uint8_t* buf, float* h, float* g) {
    const int meia = f.largura / 2;
    pares_avx2(nos, buf, meia, f.qa, f.desl);
    pares_avx2(eles, buf + meia, meia, f.qa, f.desl);

    for (int i = 0; i < f.l1; i++) {
        int32_t s = escalar_u8i8_avx2(buf, l1w + (size_t)i * f.largura, f.largura);
        float v = (float)s * f.div + l1b[i];
        h[i] = std::min(std::max(v, 0.0f), 1.0f);
        h[i + f.l1] = std::min(std::max(v * v, 0.0f), 1.0f);
    }

    for (int i = 0; i < f.l2; i += 8)
        _mm256_storeu_ps(g + i, _mm256_loadu_ps(l2b + i));
    for (int j = 0; j < f.l1 * 2; j++) {
        const float* w = l2w + (size_t)j * f.l2;
        __m256 v = _mm256_set1_ps(h[j]);
        for (int i = 0; i < f.l2; i += 8)
            _mm256_storeu_ps(g + i, _mm256_fmadd_ps(v, _mm256_loadu_ps(w + i),
                                                    _mm256_loadu_ps(g + i)));
    }

    const __m256 zero = _mm256_setzero_ps();
    const __m256 um = _mm256_set1_ps(1.0f);
    __m256 acc = _mm256_setzero_ps();
    for (int i = 0; i < f.l2; i += 8) {
        __m256 v = _mm256_min_ps(_mm256_max_ps(_mm256_loadu_ps(g + i), zero), um);
        acc = _mm256_fmadd_ps(_mm256_mul_ps(v, v), _mm256_loadu_ps(sw + i), acc);
    }
    __m128 t = _mm_add_ps(_mm256_castps256_ps128(acc), _mm256_extractf128_ps(acc, 1));
    t = _mm_add_ps(t, _mm_movehl_ps(t, t));
    t = _mm_add_ss(t, _mm_shuffle_ps(t, t, 1));
    return sb + _mm_cvtss_f32(t);
}

// ---------------------------------------------------------------- AVX-512 ---

__attribute__((target("avx512f,avx512bw,avx512vnni")))
int32_t escalar_u8i8_vnni(const uint8_t* a, const int8_t* w, int n) {
    __m512i soma = _mm512_setzero_si512();
    for (int j = 0; j < n; j += 64) {
        __m512i x = _mm512_loadu_si512((const void*)(a + j));
        __m512i y = _mm512_loadu_si512((const void*)(w + j));
        // Acumula u8 x i8 directamente em i32: nao ha' i16 pelo meio e
        // portanto nao ha' saturacao possivel.
        soma = _mm512_dpbusd_epi32(soma, x, y);
    }
    return _mm512_reduce_add_epi32(soma);
}

__attribute__((target("avx512f,avx512bw,avx512vnni")))
float cabeca_avx512(const Forma& f, const int16_t* nos, const int16_t* eles,
                    const int8_t* l1w, const float* l1b, const float* l2w,
                    const float* l2b, const float* sw, float sb,
                    uint8_t* buf, float* h, float* g) {
    const int meia = f.largura / 2;
    // O emparelhamento fica em AVX2: e' memoria, nao aritmetica, e a versao de
    // 512 do `mulhrs` com o empacotamento certo nao paga o que custa a
    // escrever.
    pares_avx2(nos, buf, meia, f.qa, f.desl);
    pares_avx2(eles, buf + meia, meia, f.qa, f.desl);

    for (int i = 0; i < f.l1; i++) {
        int32_t s = escalar_u8i8_vnni(buf, l1w + (size_t)i * f.largura, f.largura);
        float v = (float)s * f.div + l1b[i];
        h[i] = std::min(std::max(v, 0.0f), 1.0f);
        h[i + f.l1] = std::min(std::max(v * v, 0.0f), 1.0f);
    }

    for (int i = 0; i < f.l2; i++) g[i] = l2b[i];
    for (int j = 0; j < f.l1 * 2; j++) {
        const float* w = l2w + (size_t)j * f.l2;
        float v = h[j];
        for (int i = 0; i < f.l2; i++) g[i] += v * w[i];
    }
    float s = sb;
    for (int i = 0; i < f.l2; i++) {
        float v = std::min(std::max(g[i], 0.0f), 1.0f);
        s += v * v * sw[i];
    }
    return s;
}

#endif  // TEM_X86

enum Caminho { ESCALAR = 0, AVX2 = 1, AVX512 = 2 };

Caminho escolhe() {
#ifdef TEM_X86
    __builtin_cpu_init();
    if (__builtin_cpu_supports("avx512vnni") && __builtin_cpu_supports("avx512bw"))
        return AVX512;
    if (__builtin_cpu_supports("avx2") && __builtin_cpu_supports("fma")) return AVX2;
#endif
    return ESCALAR;
}

Caminho caminho = escolhe();

// ---------------------------------------------------------------- esparso ---
//
// Measured on this shape: 81.5% of the head's input bytes are zero. Four fifths
// of the first layer's work is multiplying by nothing, and the first layer is
// nine tenths of the head.
//
// So the loop is turned inside out. Dense, it walks the sixteen outputs and
// sums every input into each; sparse, it walks only the input blocks that are
// not zero and pushes each into all sixteen outputs at once. That requires the
// weights stored by INPUT BLOCK rather than by output -- for block `b`, the
// four weights of every output, side by side -- which is what `empacota` below
// builds once when the network is loaded.
//
// A block is four bytes because that is what one `dpbusd` lane consumes: the
// four inputs of the block against four weights, accumulated into one i32. The
// block is skipped when all four of its bytes are zero, which with these
// numbers is most of them.
//
// Saturation: on AVX-512 `dpbusd` accumulates straight into i32 and this is
// exact. On AVX2 the `maddubs` pairs into i16 first, and it pairs DIFFERENT
// neighbours here than it does in the dense version -- so the two can differ in
// the rare case where the intermediate saturates. That is the same trade the
// engines this shape came from already make, and the gate says which positions
// it costs, if any.

#ifdef TEM_X86

// Weights by input block: `wt[b * l1 * 4 + o * 4 + k]` is the weight of input
// `4b + k` into output `o`. Built once at load; the engine never does this in
// the search.
// The indices of the blocks that are not zero.
//
// Eight blocks at a time: compare thirty-two bytes as eight i32 against zero,
// take the mask, and walk only the bits that are set. The scan itself is a
// thirtieth of the work it saves.
__attribute__((target("avx2")))
static int blocos_nao_nulos(const uint8_t* v, int n, uint16_t* idx) {
    const __m256i zero = _mm256_setzero_si256();
    int c = 0;
    for (int b = 0; b < n / 4; b += 8) {
        __m256i x = _mm256_loadu_si256((const __m256i*)(v + 4 * b));
        // `cmpeq` marca os que SAO zero; a mascara invertida da' os outros.
        uint32_t m = (uint32_t)_mm256_movemask_ps(
            _mm256_castsi256_ps(_mm256_cmpeq_epi32(x, zero)));
        m = (~m) & 0xff;
        while (m) {
            idx[c++] = (uint16_t)(b + __builtin_ctz(m));
            m &= m - 1;
        }
    }
    return c;
}

__attribute__((target("avx2,fma")))
float cabeca_esparsa_avx2(const Forma& f, const int16_t* nos, const int16_t* eles,
                          const int8_t* wt, const float* l1b, const float* l2w,
                          const float* l2b, const float* sw, float sb,
                          uint8_t* buf, float* h, float* g) {
    const int meia = f.largura / 2;
    pares_avx2(nos, buf, meia, f.qa, f.desl);
    pares_avx2(eles, buf + meia, meia, f.qa, f.desl);

    uint16_t idx[2048 / 4];
    const int n = blocos_nao_nulos(buf, f.largura, idx);

    // Um acumulador de oito i32 por cada oito saidas. Escrito assim e nao com
    // dois fixos: dezasseis e' o que esta rede tem, nao uma propriedade da
    // forma, e um pressuposto escondido num ciclo SIMD nao da' erro, da'
    // avaliacao errada.
    const int nacc = (f.l1 + 7) / 8;
    __m256i a[8];
    for (int i = 0; i < nacc; i++) a[i] = _mm256_setzero_si256();
    const __m256i uns = _mm256_set1_epi16(1);
    const size_t passo = (size_t)f.l1 * 4;
    for (int i = 0; i < n; i++) {
        const int b = idx[i];
        // Os quatro bytes do bloco repetidos por todas as pistas: cada pista
        // trata de uma saida diferente com os seus proprios quatro pesos.
        const __m256i x = _mm256_set1_epi32(*(const int32_t*)(buf + 4 * b));
        const int8_t* w = wt + (size_t)b * passo;
        for (int j = 0; j < nacc; j++) {
            __m256i p = _mm256_maddubs_epi16(x, _mm256_loadu_si256((const __m256i*)(w + 32 * j)));
            a[j] = _mm256_add_epi32(a[j], _mm256_madd_epi16(p, uns));
        }
    }

    alignas(32) int32_t s[64];
    for (int i = 0; i < nacc; i++) _mm256_store_si256((__m256i*)(s + 8 * i), a[i]);

    for (int i = 0; i < f.l1; i++) {
        float v = (float)s[i] * f.div + l1b[i];
        h[i] = std::min(std::max(v, 0.0f), 1.0f);
        h[i + f.l1] = std::min(std::max(v * v, 0.0f), 1.0f);
    }

    for (int i = 0; i < f.l2; i += 8)
        _mm256_storeu_ps(g + i, _mm256_loadu_ps(l2b + i));
    for (int j = 0; j < f.l1 * 2; j++) {
        const float* w = l2w + (size_t)j * f.l2;
        __m256 v = _mm256_set1_ps(h[j]);
        for (int i = 0; i < f.l2; i += 8)
            _mm256_storeu_ps(g + i, _mm256_fmadd_ps(v, _mm256_loadu_ps(w + i),
                                                    _mm256_loadu_ps(g + i)));
    }

    const __m256 zero = _mm256_setzero_ps();
    const __m256 um = _mm256_set1_ps(1.0f);
    __m256 acc = _mm256_setzero_ps();
    for (int i = 0; i < f.l2; i += 8) {
        __m256 v = _mm256_min_ps(_mm256_max_ps(_mm256_loadu_ps(g + i), zero), um);
        acc = _mm256_fmadd_ps(_mm256_mul_ps(v, v), _mm256_loadu_ps(sw + i), acc);
    }
    __m128 t = _mm_add_ps(_mm256_castps256_ps128(acc), _mm256_extractf128_ps(acc, 1));
    t = _mm_add_ps(t, _mm_movehl_ps(t, t));
    t = _mm_add_ss(t, _mm_shuffle_ps(t, t, 1));
    return sb + _mm_cvtss_f32(t);
}


#endif  // TEM_X86
}  // namespace

extern "C" {

// Which path was taken, so the engine can say so and so a test can force one.
int cabeca_caminho(void) { return (int)caminho; }
void cabeca_forca(int c) { caminho = (Caminho)c; }

float cabeca_avalia(int largura, int l1, int l2, int qa, int desl, float div,
                    const int16_t* nos, const int16_t* eles,
                    const int8_t* l1w, const float* l1b, const float* l2w,
                    const float* l2b, const float* sw, float sb) {
    Forma f{largura, l1, l2, qa, desl, div};
    // Espaco de trabalho na pilha. O maior que isto precisa de ser e' a
    // largura do acumulador, e nenhuma rede nossa passa de 2048.
    uint8_t buf[2048];
    float h[128];
    float g[128];
#ifdef TEM_X86
    if (caminho == AVX512)
        return cabeca_avx512(f, nos, eles, l1w, l1b, l2w, l2b, sw, sb, buf, h, g);
    if (caminho == AVX2)
        return cabeca_avx2(f, nos, eles, l1w, l1b, l2w, l2b, sw, sb, buf, h, g);
#endif
    return cabeca_escalar(f, nos, eles, l1w, l1b, l2w, l2b, sw, sb, buf, h, g);
}

void cabeca_empacota(int largura, int l1, const int8_t* origem, int8_t* destino) {
    for (int b = 0; b < largura / 4; b++) {
        for (int o = 0; o < l1; o++) {
            for (int k = 0; k < 4; k++) {
                destino[(size_t)b * l1 * 4 + o * 4 + k] = origem[(size_t)o * largura + 4 * b + k];
            }
        }
    }
}


float cabeca_avalia_esparsa(int largura, int l1, int l2, int qa, int desl,
                                       float div, const int16_t* nos, const int16_t* eles,
                                       const int8_t* wt, const float* l1b, const float* l2w,
                                       const float* l2b, const float* sw, float sb) {
#ifdef TEM_X86
    Forma f{largura, l1, l2, qa, desl, div};
    uint8_t buf[2048];
    float h[128];
    float g[128];
    if (caminho != ESCALAR)
        return cabeca_esparsa_avx2(f, nos, eles, wt, l1b, l2w, l2b, sw, sb, buf, h, g);
#endif
    // Sem SIMD nao ha' nada a ganhar em saltar blocos, e a versao densa esta'
    // la' para isso -- quem chamar isto sem AVX2 tem de usar a outra.
    (void)largura; (void)l1; (void)l2; (void)qa; (void)desl; (void)div;
    (void)nos; (void)eles; (void)wt; (void)l1b; (void)l2w; (void)l2b; (void)sw;
    return sb;
}

}  // extern "C"
