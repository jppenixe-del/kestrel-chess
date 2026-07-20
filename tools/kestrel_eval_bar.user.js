// ==UserScript==
// @name         Kestrel eval bar on Lichess
// @namespace    kestrel
// @version      0.2
// @description  Desenha uma regua de avaliacao (estilo chess.com) a esquerda do tabuleiro Lichess quando estas a ver um jogo do KestrelStrike, lendo o score em tempo real do web viewer local da ponte
// @match        https://lichess.org/*
// @grant        GM_xmlhttpRequest
// @connect      10.0.0.2
// @connect      localhost
// ==/UserScript==
//
// Setup:
//   1. Instala o Tampermonkey (Chrome/Firefox/Edge)
//   2. Cria um novo user script e cola isto
//   3. Ajusta BRIDGE_URL abaixo para o endereco onde o weblog da ponte
//      esta acessivel da tua rede (napoleon@10.0.0.2 tem o weblog em
//      /mnt/c/kestrel_bot/bridge/weblog/, servido via http.server na
//      porta que definires). Se estas no MESMO PC do Kestrel, usa
//      http://localhost:PORTA.
//   4. Quando abres uma partida onde o KestrelStrike joga, a regua
//      aparece automaticamente a esquerda do tabuleiro.
//
// A regua funciona como no chess.com: barra vertical branca/preta a
// mostrar quem esta melhor pela avaliacao do proprio Kestrel, actualizada
// a cada lance jogado por ele.

(function () {
    'use strict';

    const BRIDGE_URL = 'http://10.0.0.2:8768/eval.jsonl';  // ajustar conforme setup do weblog
    const POLL_MS = 1500;
    const BOT_USERNAME = 'kestrelstrike';

    let currentGameId = null;
    let barEl = null;
    let labelEl = null;

    function extractGameId() {
        // URL de partida no Lichess: /GAMEID ou /GAMEID/COLOR
        const m = location.pathname.match(/^\/([A-Za-z0-9]{8})(\/(white|black))?$/);
        return m ? m[1] : null;
    }

    function pageMentionsBot() {
        // Verifica se a pagina inclui o BOT_USERNAME em algum lado
        // (nome do jogador, header, etc.) -- so mostra a regua quando
        // e' uma partida onde o bot joga.
        return document.body.textContent.toLowerCase().includes(BOT_USERNAME);
    }

    function ensureBar() {
        if (barEl && barEl.isConnected) return;
        const boardContainer = document.querySelector('cg-container') ||
                               document.querySelector('.cg-wrap') ||
                               document.querySelector('.round__app__board');
        if (!boardContainer) return;

        const wrap = document.createElement('div');
        wrap.id = 'kestrel-eval-bar-wrap';
        wrap.style.cssText = 'position:absolute;left:-32px;top:0;bottom:0;width:22px;display:flex;flex-direction:column;align-items:center;justify-content:flex-start;pointer-events:none;font-family:-apple-system,BlinkMacSystemFont,sans-serif;';

        labelEl = document.createElement('div');
        labelEl.id = 'kestrel-eval-label';
        labelEl.style.cssText = 'font-size:11px;font-weight:600;color:#eee;background:#333;padding:2px 4px;border-radius:2px;margin-bottom:2px;min-width:36px;text-align:center;';
        labelEl.textContent = '--';
        wrap.appendChild(labelEl);

        barEl = document.createElement('div');
        barEl.id = 'kestrel-eval-bar';
        barEl.style.cssText = 'flex:1;width:22px;background:#000;border:1px solid #555;position:relative;overflow:hidden;';
        // fill vem de dentro; branco domina desde o fundo
        const fill = document.createElement('div');
        fill.id = 'kestrel-eval-fill';
        fill.style.cssText = 'position:absolute;left:0;right:0;bottom:0;background:#fff;transition:height 0.4s ease;height:50%;';
        barEl.appendChild(fill);
        wrap.appendChild(barEl);

        // Garante posicionamento relative no container do tabuleiro
        const target = boardContainer.parentElement || boardContainer;
        if (getComputedStyle(target).position === 'static') {
            target.style.position = 'relative';
        }
        target.appendChild(wrap);
    }

    function removeBar() {
        const el = document.getElementById('kestrel-eval-bar-wrap');
        if (el) el.remove();
        barEl = null;
        labelEl = null;
    }

    function cpToWinrate(cp) {
        // mesma sigmoide que a ponte usa para o chat -- coerente
        return 1.0 / (1.0 + Math.exp(-cp / 180.0));
    }

    function updateBar(entry) {
        if (!barEl || !labelEl) return;
        const fill = document.getElementById('kestrel-eval-fill');
        if (entry === null || entry.score_cp === null || entry.score_cp === undefined) {
            labelEl.textContent = '--';
            if (fill) fill.style.height = '50%';
            return;
        }
        const cp = entry.score_cp;
        // mate score detection (motor devolve ~100000 - N*100)
        let text, wr;
        if (Math.abs(cp) >= 99000) {
            const mateN = Math.max(1, Math.round((100000 - Math.abs(cp)) / 100));
            text = (cp > 0 ? '+' : '-') + '#' + mateN;
            wr = cp > 0 ? 1.0 : 0.0;
        } else {
            const val = cp / 100.0;
            text = (val > 0 ? '+' : '') + val.toFixed(2);
            wr = cpToWinrate(cp);
        }
        labelEl.textContent = text;
        labelEl.style.color = cp >= 0 ? '#0a0' : '#e33';
        if (fill) fill.style.height = (wr * 100).toFixed(1) + '%';
    }

    let lastKnownPly = -1;
    function poll() {
        const gid = extractGameId();
        if (gid !== currentGameId) {
            currentGameId = gid;
            lastKnownPly = -1;
            removeBar();
        }
        if (!gid || !pageMentionsBot()) {
            removeBar();
            return;
        }
        ensureBar();
        // Le' o eval.jsonl completo (ficheiro pequeno, append-only) e
        // fica com a entrada mais recente para este game_id.
        GM_xmlhttpRequest({
            method: 'GET',
            url: BRIDGE_URL + '?_=' + Date.now(),
            timeout: 5000,
            onload: (resp) => {
                if (resp.status !== 200) return;
                const lines = resp.responseText.trim().split('\n');
                let latest = null;
                for (let i = lines.length - 1; i >= 0; i--) {
                    try {
                        const obj = JSON.parse(lines[i]);
                        if (obj.game_id === gid) {
                            latest = obj;
                            break;
                        }
                    } catch (_) { /* linha corrompida, ignora */ }
                }
                if (latest && latest.ply > lastKnownPly) {
                    lastKnownPly = latest.ply;
                    updateBar(latest);
                }
            },
            onerror: () => {},
            ontimeout: () => {},
        });
    }

    setInterval(poll, POLL_MS);
    poll();
})();
