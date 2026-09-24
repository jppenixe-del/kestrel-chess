# KestrelStrike -- construcao.
#
#   make                 constroi para a maquina onde estiver (native)
#   make ARCH=bmi2       x86-64 com AVX2 e PEXT -- Intel desde Haswell, AMD desde Zen 3
#   make ARCH=avx2       x86-64 com AVX2, SEM PEXT -- corre bem em quase todo o x86
#   make ARCH=avx512     x86-64 com AVX-512   -- so' onde o processador o tem
#   make ARCH=sse41      x86-64 antigo, sem AVX
#   make todos           os quatro de uma vez, com o nome da arquitectura no fim
#
# O PEXT SEPARA O BMI2 DO AVX2. Os AMD Zen 1 e Zen 2 tem a instrucao, mas em
# microcodigo, dezenas de vezes mais lenta; um binario com PEXT corre neles
# muito abaixo do que devia. Quem nao sabe o processador escolhe o `avx2`.
#
# AS MACROS DA SIMD SAO OBRIGATORIAS E NAO SE VEEM.
#
# O leitor da rede em `vendor/nnue` NAO conta com a auto-vectorizacao do
# compilador: as rotinas estao escritas em intrinsecos atras de
# `#if defined(USE_AVX2)` e afins. Sem as macros o motor compila, corre, joga --
# e faz a rede em ESCALAR. Medido: 50.393 nos por segundo contra 419.883.
# Oito vezes. E nao da' erro nenhum; da' um motor que ve' menos quatro
# profundidades no mesmo tempo e ninguem percebe porque'.
#
# Por isso cada arquitectura define as SUAS macros, e nenhuma delas e' opcional.

CXX      ?= g++
NOME     ?= kestrelstrike
ARCH     ?= native
EXE      ?= $(NOME)

# A REDE DENTRO DO EXECUTAVEL.
#
# Com a rede embebida o motor joga sozinho: sem `EvalFile`, sem ficheiro ao
# lado, sem maneira de alguem correr o motor com a rede errada. E' tambem a
# unica forma de distribuir um so' ficheiro para a CCRL.
#
# O `incbin` resolve o caminho a partir do directorio onde o make corre, por
# isso a rede tem de estar na raiz do repositorio. Se nao estiver, constroi-se
# na mesma -- mas o motor passa a exigir `EvalFile`, e o aviso abaixo diz-lo.
REDE     ?= ks-cf796d1f923d.nnue
ifeq ($(wildcard $(REDE)),)
  EMBEBE  = -DNNUE_EMBEDDING_OFF
else
  EMBEBE  = -DKESTREL_REDE_EMBEBIDA
endif

COMUM    = -std=c++20 -O3 -DNDEBUG $(EMBEBE) -Ivendor -Isrc \
           -funroll-loops -fno-exceptions -fno-rtti
LDFLAGS  = -lpthread

ifeq ($(ARCH),avx512)
  SIMD = -DUSE_AVX512 -DUSE_AVX2 -DUSE_SSE41 -DUSE_SSSE3 -DUSE_SSE2 -DUSE_POPCNT -DUSE_PEXT \
         -mavx512f -mavx512bw -mavx2 -mbmi -mbmi2 -msse4.1 -mssse3 -mpopcnt
else ifeq ($(ARCH),bmi2)
  SIMD = -DUSE_AVX2 -DUSE_SSE41 -DUSE_SSSE3 -DUSE_SSE2 -DUSE_POPCNT -DUSE_PEXT \
         -mavx2 -mbmi -mbmi2 -msse4.1 -mssse3 -mpopcnt
else ifeq ($(ARCH),avx2)
  SIMD = -DUSE_AVX2 -DUSE_SSE41 -DUSE_SSSE3 -DUSE_SSE2 -DUSE_POPCNT \
         -mavx2 -mbmi -msse4.1 -mssse3 -mpopcnt
else ifeq ($(ARCH),sse41)
  SIMD = -DUSE_SSE41 -DUSE_SSSE3 -DUSE_SSE2 -DUSE_POPCNT \
         -msse4.1 -mssse3 -mpopcnt
else
  SIMD = -DUSE_AVX2 -DUSE_SSE41 -DUSE_SSSE3 -DUSE_SSE2 -DUSE_POPCNT -DUSE_PEXT \
         -march=native -mtune=native
endif

# O SUBSTRATO. Nao se compila a camada de motor do Stockfish -- `search.cpp`,
# `engine.cpp`, `movepick.cpp`, `thread.cpp`, `timeman.cpp`, `uci.cpp`. A busca
# e' nossa e a tabela de transposicao tambem, e a deles quer uma interface
# (`probe`, `new_search`, `hashfull`) que a nossa nao tem nem deve ter.
SUBSTRATO = vendor/attacks.cpp vendor/bitboard.cpp vendor/memory.cpp vendor/misc.cpp \
            vendor/movegen.cpp vendor/position.cpp vendor/score.cpp vendor/evaluate.cpp \
            vendor/ucioption.cpp \
            vendor/nnue/network.cpp vendor/nnue/nnue_accumulator.cpp vendor/nnue/nnue_misc.cpp \
            vendor/nnue/features/half_ka_v2_hm.cpp vendor/nnue/features/full_threats.cpp \
            vendor/nnue/features/pp_3wide.cpp vendor/syzygy/tbprobe.cpp

NOSSO     = src/busca.cpp src/tt.cpp src/aval.cpp src/uci_laco.cpp src/uci_compat.cpp

$(EXE): $(SUBSTRATO) $(NOSSO)
	$(CXX) $(COMUM) $(SIMD) -o $@ $^ $(LDFLAGS)

todos:
	$(MAKE) ARCH=avx512 EXE=$(NOME)-avx512
	$(MAKE) ARCH=bmi2   EXE=$(NOME)-bmi2
	$(MAKE) ARCH=avx2   EXE=$(NOME)-avx2
	$(MAKE) ARCH=sse41  EXE=$(NOME)-sse41

# O WINDOWS, construido a partir do Linux com o mingw-w64.
#
# Estatico, para correr sem DLLs ao lado. E com a pilha de 8 MB: no Windows uma
# thread nasce com a pilha que o executavel declara, 1 MB por omissao, e a
# recursao da busca chega perto de 1,6 MB no pior caso (MAX_PLY). No Linux
# nasce com 8 MB, que e' o que o Stockfish tambem pede.
MINGW    ?= x86_64-w64-mingw32-g++
WIN_LD    = -static -lpthread -Wl,--stack,8388608

windows:
	$(MAKE) CXX=$(MINGW) ARCH=avx512 EXE=$(NOME)-avx512.exe LDFLAGS="$(WIN_LD)"
	$(MAKE) CXX=$(MINGW) ARCH=bmi2   EXE=$(NOME)-bmi2.exe   LDFLAGS="$(WIN_LD)"
	$(MAKE) CXX=$(MINGW) ARCH=avx2   EXE=$(NOME)-avx2.exe   LDFLAGS="$(WIN_LD)"
	$(MAKE) CXX=$(MINGW) ARCH=sse41  EXE=$(NOME)-sse41.exe  LDFLAGS="$(WIN_LD)"

limpo:
	rm -f $(NOME) $(NOME)-avx512 $(NOME)-bmi2 $(NOME)-avx2 $(NOME)-sse41
	rm -f $(NOME)-avx512.exe $(NOME)-bmi2.exe $(NOME)-avx2.exe $(NOME)-sse41.exe

.PHONY: todos windows limpo
