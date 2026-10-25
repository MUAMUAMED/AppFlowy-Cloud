import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import test from 'node:test';
import { parseHTML } from 'linkedom';

const source = readFileSync(new URL('../content.js', import.meta.url), 'utf8');
function setup(html) {
  const { window, document } = parseHTML(`<html><body>${html}</body></html>`);
  const context = vm.createContext({
    document, Node: window.Node, MutationObserver: window.MutationObserver,
    setTimeout, clearTimeout, console, Date, location: { href: 'https://www.grancursosonline.com.br/aluno/curso/video/test' },
    XMLSerializer: class { serializeToString(node) { return node.outerHTML; } },
    chrome: { runtime: { onMessage: { addListener() {} } } },
  });
  vm.runInContext(source, context);
  return { document, run: (code) => vm.runInContext(code, context) };
}

test('summary ignores hidden stale drawers and waits for delayed body content', async () => {
  const { document, run } = setup(`
    <button aria-label="Selecionar o artefato de Resumo">Resumo</button>
    <section hidden><h2 id="player-overlay-title">Resumo</h2><p>Texto antigo que não pertence ao material da aula aberta atualmente.</p></section>
    <section id="active"><h2 id="player-overlay-title">Resumo</h2></section>`);
  const pending = run('extractTextArtifact(["resumo"])');
  setTimeout(() => {
    document.querySelector('#active').insertAdjacentHTML('beforeend', '<h3>Função modular</h3><p>Uma função modular envolve o <strong>valor absoluto</strong> e deve ser estudada por intervalos.</p>');
  }, 650);
  const text = await pending;
  assert.match(text, /\*\*valor absoluto\*\*/);
  assert.match(text, /### Função modular/);
  assert.doesNotMatch(text, /Texto antigo/);
});

test('title-only material never passes readiness', async () => {
  const { run } = setup('<section><h2 id="player-overlay-title">Resumo</h2></section>');
  await assert.rejects(run('waitForStableArtifact(() => null, 250, 100)'), /não carregou/);
  assert.equal(run('artifactPanelText(getArtifactPanel(["resumo"]))'), '');
});

test('Gran error message is reported instead of being imported as summary', async () => {
  const { run } = setup('<button>Resumo</button><section><h2 id="player-overlay-title">Resumo</h2><p>Não foi possível carregar o resumo desta aula. Tente novamente mais tarde.</p></section>');
  await assert.rejects(run('extractTextArtifact(["resumo"])'), /Gran exibiu uma falha/);
});

test('artifact selection distinguishes summary from old summary menu and pocket review', () => {
  const { run } = setup(`<button>Resumo e exercícios</button><button>Resumo de bolso</button><button title="Resumo da aula">Ler</button>`);
  assert.equal(run('findArtifactButton(["resumo"]).getAttribute("title")'), 'Resumo da aula');
});

test('flashcards preserve all paragraphs on both faces and support labelled sides', () => {
  const { run } = setup(`<section><h2>Flashcards</h2><span>Cartão 2 de 15</span>
    <div data-side="front"><p>Qual é a definição?</p><p>Considere o domínio real.</p></div>
    <div data-side="back"><p>É o valor absoluto.</p><p>Nunca é negativo.</p></div></section>`);
  const card = run('getFlashcard(getFlashcardsPanel())');
  assert.equal(card.total, 15);
  assert.match(card.front, /domínio real/);
  assert.match(card.back, /Nunca é negativo/);
});

test('navigation supports accessible labels without FontAwesome icons', () => {
  const { run } = setup('<section><h2>Flashcards</h2><button aria-label="Próximo cartão">→</button></section>');
  assert.equal(run('flashcardNavigation(getFlashcardsPanel(), "next").getAttribute("aria-label")'), 'Próximo cartão');
});

test('map collector waits for SVG and skips decorative error images', async () => {
  const { document, run } = setup(`<button title="Mapa mental">Mapa</button><section>
    <h2 id="player-overlay-title">Mapa mental</h2><img src="https://cdn.test/error-illustration.svg" width="400" alt="Erro" />
    <div id="map"></div></section>`);
  assert.equal(run('getMindMapImages(getArtifactPanel(["mapa mental"])).length'), 0);
  const pending = run('extractMindMaps()');
  setTimeout(() => {
    document.querySelector('#map').innerHTML = '<svg class="markmap"><g class="markmap-node" data-path="1"><text>Função modular</text></g></svg>';
  }, 650);
  const maps = await pending;
  assert.equal(maps.length, 1);
  assert.match(decodeURIComponent(maps[0]), /Função modular/);
  assert.doesNotMatch(maps[0], /error-illustration/);
});

test('wait handles transient missing counters and attribute-only updates', async () => {
  const { document, run } = setup('<span id="counter" data-ready="false"></span>');
  const pending = run('waitFor(() => { if (document.querySelector("#counter").dataset.ready !== "true") throw Error("transition"); return true; }, 1500)');
  setTimeout(() => { document.querySelector('#counter').dataset.ready = 'true'; }, 200);
  assert.equal(await pending, true);
});

test('collects exactly 15 cards across asynchronous navigation', async () => {
  const { document, run } = setup('<section><h2>Flashcards</h2><div id="deck"></div><button aria-label="Próximo cartão">→</button></section>');
  let index = 1;
  const render = () => {
    document.querySelector('#deck').innerHTML = `<span>Cartão ${index} de 15</span><div data-side="front">Pergunta ${index}</div><div data-side="back">Resposta ${index}</div>`;
  };
  render();
  document.querySelector('button').addEventListener('click', () => {
    document.querySelector('#deck').innerHTML = '';
    setTimeout(() => { index += 1; render(); }, 30);
  });
  const result = await run('extractFlashcards()');
  assert.equal(result.cards.length, 15);
  assert.equal(result.cards[0].front, 'Pergunta 1');
  assert.equal(result.cards[14].back, 'Resposta 15');
  assert.equal(new Set(result.cards.map((card) => card.front)).size, 15);
});
