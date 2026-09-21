// Compila a cabeca escrita a` mao.
//
// O `cc` escolhe o compilador da maquina e as flags de optimizacao; os
// conjuntos de instrucoes NAO vem daqui -- cada funcao traz o seu
// `__attribute__((target(...)))` e a escolha faz-se em execucao, para o mesmo
// binario correr numa maquina com AVX-512 e noutra sem ele.
fn main() {
    compila_leitor();

    println!("cargo:rerun-if-changed=src/cabeca.cpp");
    cc::Build::new()
        .cpp(true)
        .file("src/cabeca.cpp")
        .opt_level(3)
        .flag_if_supported("-std=c++17")
        .warnings(false)
        .compile("cabeca");
}


/// Compila o leitor da rede a partir da arvore, mais a nossa cola.
///
/// `nnue_vendor/` e' trabalho derivado: ver o README de la' e a linha no
/// NOTICES.md. `src/cola_rede.cpp` e' nosso -- conduz a posicao deles a par da
/// nossa com make/unmake, que e' o que faz o acumulador seguir a busca em vez
/// de ser reconstruido a cada no'.
fn compila_leitor() {
    let raiz = std::path::Path::new("nnue_vendor");
    if !raiz.join("nnue/network.cpp").exists() {
        return;
    }
    // A rede por omissao do leitor entra por `incbin` e nao a usamos -- a nossa
    // vem pelo EvalFile. Um ficheiro vazio onde ele a procura deixa-o compilar
    // sem arrastar cem megabytes que nunca se leem.
    // O NOME da rede por omissao MUDA a cada actualizacao do leitor, e ate'
    // 20-09 estava cravado aqui: a actualizacao trouxe `nn-134a887f4c8f.nnue`,
    // o ficheiro vazio continuava a chamar-se `nn-1a298aa575a0.nnue`, e o
    // `incbin` nao achava nada. Passa a ler-se o nome DA FONTE, que e' quem
    // sabe qual e', em vez de o repetir aqui e esperar que ninguem lhe mexa.
    for f in ["nnue/network.cpp", "nnue/nnue_misc.cpp", "evaluate.h", "evaluate.cpp"] {
        let cam = raiz.join(f);
        if let Ok(t) = std::fs::read_to_string(&cam) {
            for pedaco in t.split("nn-").skip(1) {
                if let Some(fim) = pedaco.find(".nnue") {
                    let nome = format!("nn-{}.nnue", &pedaco[..fim]);
                    if nome.len() < 40 {
                        let vazio = raiz.join(&nome);
                        if !vazio.exists() {
                            let _ = std::fs::write(&vazio, b"");
                            println!("cargo:warning=rede por omissao vazia criada: {}", nome);
                        }
                    }
                }
            }
        }
    }
    let mut b = cc::Build::new();
    b.cpp(true).opt_level(3).warnings(false)
        .flag_if_supported("-std=c++17")
        .flag_if_supported("-fno-exceptions")
        .include(raiz)
        .define("NDEBUG", None)
        .define("IS_64BIT", None)
        .define("USE_AVX2", None).flag_if_supported("-mavx2")
        .define("USE_POPCNT", None).flag_if_supported("-mpopcnt")
        .define("USE_SSE41", None).flag_if_supported("-msse4.1")
        .define("USE_SSSE3", None).flag_if_supported("-mssse3")
        .define("USE_SSE2", None).flag_if_supported("-msse2")
        .flag_if_supported("-mbmi");
    // MEDIDO E REJEITADO (2026-09-05): `USE_PEXT` mais `-mbmi2`.
    //
    // O codigo vendorizado suporta-o (`USE_PEXT` em types.h) e a ideia era
    // trocar a tabela magica pela instrucao. Quatro corridas alternadas de cada
    // lado, arvore identica nos dois (517.054 nos, o mesmo numero): mediana de
    // 5.542 milhoes de ciclos com PEXT contra 5.308 sem, +4,4%. O minimo, que e'
    // a corrida menos disputada e portanto a medida mais limpa, diz o mesmo:
    // 5.439 contra 5.207.
    //
    // Fica registado para ninguem voltar a tentar sem primeiro medir NA MAQUINA
    // ONDE VAI CORRER: num Zen 3 o PEXT e' rapido, e neste processador nao e'.
    for d in [raiz.to_path_buf(), raiz.join("nnue"), raiz.join("nnue/features"),
              raiz.join("nnue/layers"), raiz.join("syzygy")] {
        if let Ok(it) = std::fs::read_dir(&d) {
            for e in it.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "cpp").unwrap_or(false) {
                    println!("cargo:rerun-if-changed={}", p.display());
                    b.file(&p);
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=src/cola_rede.cpp");
    b.file("src/cola_rede.cpp");
    // Os contadores do acumulador, NOSSOS. Estavam enxertados dentro do
    // `nnue_accumulator.cpp` deles e a actualizacao do vendor levou-os; agora
    // vivem em ficheiro proprio para isso nao voltar a acontecer.
    println!("cargo:rerun-if-changed=nnue_vendor/conta_h2k.cpp");
    b.file("nnue_vendor/conta_h2k.cpp");
    println!("cargo:rerun-if-changed=src/busca_rapida.cpp");
    b.file("src/busca_rapida.cpp");
    b.compile("leitor");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
