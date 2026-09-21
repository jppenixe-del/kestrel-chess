// A busca em C++, peca a peca, comecando pelo unico sitio onde o SIMD se
// aplica de facto.
//
// O alfa-beta nao se vectoriza: e' uma caminhada de ramos dependentes, e nao
// ha' instrucao nenhuma que acelere um `if score >= beta`. Mas ha' duas coisas
// dentro dele com forma de CICLO SOBRE UM VECTOR, e essas vectorizam-se:
//   pick        -- o maximo de um vector de inteiros, 5,2% do tempo
//   score_moves -- pontuar N lances, 7,6%
//
// Este ficheiro comeca pelo primeiro. A regra e' que o resultado tem de ser
// IDENTICO ao da versao em Rust, incluindo os empates: o Rust usa `>` estrito,
// portanto fica com o PRIMEIRO maximo, e uma versao que ficasse com o ultimo
// mudava a ordem dos lances e com ela a arvore inteira -- e ai' ja' nao
// estariamos a medir velocidade, estariamos a medir outra busca.
//
// Por isso sao duas passagens: a primeira acha o valor maximo com AVX2, a
// segunda acha o primeiro indice que o tem. A segunda passagem custa pouco --
// para quando encontra -- e garante a identidade.

#include <cstdint>
#include <cstddef>
#if defined(__AVX2__)
#include <immintrin.h>
#endif

extern "C" size_t h2k_argmax_i32(const int32_t* v, size_t n, size_t inicio) {
    if (inicio >= n) return inicio;
    const int32_t* p = v + inicio;
    size_t m = n - inicio;
    int32_t melhor = p[0];

#if defined(__AVX2__)
    size_t i = 0;
    if (m >= 8) {
        __m256i vmax = _mm256_loadu_si256((const __m256i*)p);
        i = 8;
        for (; i + 8 <= m; i += 8) {
            __m256i x = _mm256_loadu_si256((const __m256i*)(p + i));
            vmax = _mm256_max_epi32(vmax, x);
        }
        // Reducao horizontal: 8 -> 4 -> 2 -> 1.
        __m128i lo = _mm256_castsi256_si128(vmax);
        __m128i hi = _mm256_extracti128_si256(vmax, 1);
        __m128i r = _mm_max_epi32(lo, hi);
        r = _mm_max_epi32(r, _mm_shuffle_epi32(r, 0x4E));
        r = _mm_max_epi32(r, _mm_shuffle_epi32(r, 0xB1));
        melhor = _mm_cvtsi128_si32(r);
    }
    for (; i < m; ++i) {
        if (p[i] > melhor) melhor = p[i];
    }
#else
    for (size_t i = 1; i < m; ++i) {
        if (p[i] > melhor) melhor = p[i];
    }
#endif

    // O PRIMEIRO com esse valor, que e' o que o `>` estrito do Rust escolhe.
    for (size_t i = 0; i < m; ++i) {
        if (p[i] == melhor) return inicio + i;
    }
    return inicio;
}
