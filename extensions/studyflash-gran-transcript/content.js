// Gran occasionally removes a node while an overlay is being rendered.  In
// that short interval textContent/innerText can be null (not just undefined),
// so always coerce before normalising it.
const normalize = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();
const artifactKey = (value) => normalize(value).normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();

function isVisible(element) {
  if (!element?.isConnected) return false;
  for (let node = element; node?.nodeType === 1; node = node.parentElement) {
    if (node.hidden || node.getAttribute('aria-hidden') === 'true') return false;
    const style = typeof getComputedStyle === 'function' ? getComputedStyle(node) : node.style;
    if (style?.display === 'none' || style?.visibility === 'hidden') return false;
  }
  return true;
}

// Poll as well as observing the DOM: image decoding, CSS visibility, and
// carousel state changes do not necessarily add/remove text nodes.
async function waitForStableArtifact(read, timeout = 30_000, stableMs = 800) {
  const deadline = Date.now() + timeout;
  let last = '';
  let stableSince = Date.now();
  while (Date.now() < deadline) {
    const value = read();
    const signature = value ? JSON.stringify(value) : '';
    if (!signature || signature !== last) stableSince = Date.now();
    if (signature && signature === last && Date.now() - stableSince >= stableMs) return value;
    last = signature;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error('O material não carregou conteúdo completo. Nenhuma página vazia será enviada.');
}

function findSummaryButton() {
  const explicitButton = document.querySelector('button[aria-label="Resumo e exercícios"]');
  if (explicitButton) return explicitButton;

  return [...document.querySelectorAll('button')].find((element) => {
    const text = normalize(element.innerText || element.textContent);
    const label = normalize(element.getAttribute('aria-label'));
    return /resumo\s+e\s+exerc[ií]cios/i.test(`${text} ${label}`);
  });
}

function findTranscriptMenuItem() {
  return [...document.querySelectorAll('[role="menuitem"], button, a, div')].find((element) => {
    const text = normalize(element.innerText || element.textContent);
    return text === 'Transcrição' || text.startsWith('Acompanhe a aula com transcrições');
  });
}

async function waitFor(getElement, timeout = 8_000, timeoutMessage = 'O conteúdo demorou para abrir. Tente novamente.') {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    // React briefly removes the old counter/card before rendering the next.
    // A transient missing counter must not abort the whole collection.
    try {
      const element = getElement();
      if (element) return element;
    } catch { /* The view is still changing. */ }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(timeoutMessage);
}

function getTranscriptDialog() {
  // Current Gran lessons render this artifact in a drawer, not a native
  // dialog. Keep the dialog fallback for older lesson pages.
  return getArtifactPanel(['transcrição']);
}

function getReadyTranscriptDialog() {
  const dialog = getTranscriptDialog();
  if (!dialog) return null;

  const text = normalize(dialog.innerText || dialog.textContent);
  const stillLoading = /carregando informa[cç][õo]es/i.test(text);
  const hasEnoughContent = text.length > 350;

  return !stillLoading && hasEnoughContent ? dialog : null;
}

function transcriptToText(dialog, lessonTitle) {
  const lines = [
    `Aula: ${lessonTitle}`,
    `Origem: ${location.href}`,
    `Extraído em: ${new Date().toLocaleString('pt-BR')}`,
    '',
  ];

  const contentNodes = dialog.querySelectorAll('h1, h2, h3, h4, p, li');
  const seen = new Set();

  let pendingStudyTip = false;
  for (const node of contentNodes) {
    const text = normalize(node.innerText || node.textContent);
    if (!text || text === 'Transcrição' || text === 'Expandir' || text === 'Fechar' || seen.has(text)) continue;
    seen.add(text);

    if (/^H[1-4]$/.test(node.tagName)) {
      pendingStudyTip = false;
      lines.push(`\n${'#'.repeat(Number(node.tagName.slice(1)) + 1)} ${inlineMarkdown(node)}\n`);
    } else if (node.tagName === 'LI') {
      pendingStudyTip = false;
      lines.push(`- ${inlineMarkdown(node)}`);
    } else if (/^💡?\s*Cai em Prova!?$/i.test(text)) {
      pendingStudyTip = true;
      lines.push('> [!TIP] Cai em Prova!');
    } else if (pendingStudyTip) {
      lines.push(`> ${inlineMarkdown(node)}`);
      pendingStudyTip = false;
    } else {
      lines.push(inlineMarkdown(node));
    }
  }

  if (lines.length <= 4) {
    const fallback = normalize(dialog.innerText || dialog.textContent)
      .replace(/^Transcrição\s*(Expandir)?\s*(Fechar)?\s*/i, '');
    if (fallback) lines.push(fallback);
  }

  return lines.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
}

// Keep the semantic formatting that Gran has already rendered.  The backend
// turns this compact Markdown into native AppFlowy heading/list/callout blocks.
function inlineMarkdown(node) {
  const visit = (current) => {
    if (current.nodeType === Node.TEXT_NODE) return current.nodeValue || '';
    if (current.tagName?.toUpperCase() === 'BR') return '\n';
    const text = [...current.childNodes].map(visit).join('');
    const tag = current.tagName?.toUpperCase();
    return ['STRONG', 'B'].includes(tag) ? `**${text}**` : text;
  };
  return normalize(visit(node));
}

async function extractTranscript() {
  const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
  let dialog = null;
  try {
    // New Gran UI exposes the transcription directly in the revision centre.
    // Older lessons still keep it behind the "Resumo e exercícios" menu.
    const directButton = findArtifactButton(['transcrição']);
    if (directButton) {
      directButton.click();
    } else {
      const summaryButton = await waitFor(findSummaryButton, 8_000, 'Não encontrei a opção de transcrição nesta aula.');
      summaryButton.click();
      const menuItem = await waitFor(findTranscriptMenuItem, 8_000, 'Não encontrei a opção de transcrição nesta aula.');
      menuItem.click();
    }

    dialog = await waitFor(getReadyTranscriptDialog, 30_000, 'A transcrição demorou para carregar. Tente novamente.');
    const text = transcriptToText(dialog, lessonTitle);

    if (text.length < 80) throw new Error('A transcrição retornou vazia.');
    return { text, title: lessonTitle, transcript: text };
  } finally {
    // A failed transcription must not leave its modal covering the remaining
    // materials (which used to make the mind-map step time out afterwards).
    await closeArtifactOverlay(dialog || getTranscriptDialog());
  }
}

function questionToText(question) {
  const questionId = normalize(question.querySelector('.ds-question__header__top__id')?.textContent) || 'Questão';
  const subject = normalize(question.querySelector('.ds-question__header__top__subject')?.textContent);
  const metadata = [...question.querySelectorAll('.ds-question__header__bottom__info')]
    .map((element) => normalize(element.textContent))
    .filter(Boolean);
  const statement = normalize(question.querySelector('.ds-question__body__statement')?.innerText);
  const options = [...question.querySelectorAll('.ds-question__body__options__option')]
    .map((element) => normalize(element.innerText))
    .filter(Boolean);

  if (!statement || options.length === 0) throw new Error('Não encontrei o enunciado ou as alternativas nesta questão.');

  return [
    `Questão: ${questionId}`,
    subject && `Assunto: ${subject}`,
    `Origem: ${location.href}`,
    `Extraído em: ${new Date().toLocaleString('pt-BR')}`,
    '',
    'ENUNCIADO',
    statement,
    '',
    'ALTERNATIVAS',
    ...options.map((option) => `- ${option}`),
    '',
    'GABARITO: não informado (a extensão não responde questões).',
    '',
    metadata.length ? `METADADOS\n${metadata.join('\n')}` : '',
  ].filter(Boolean).join('\n') + '\n';
}

function extractFixationQuestion() {
  const question = document.querySelector('.ds-question');
  if (!question) throw new Error('Abra uma questão individual do Gran Questões para extrair.');

  const text = questionToText(question);
  const title = normalize(question.querySelector('.ds-question__header__top__id')?.textContent) || 'questao-gran';
  return { text, title };
}

function getFixationDialog() {
  return [...document.querySelectorAll('[role="dialog"], dialog')].find((dialog) =>
    /Exercícios de Fixação/.test(dialog.innerText || ''),
  );
}

async function ensureFixationDialog() {
  const openDialog = getFixationDialog();
  if (openDialog) return openDialog;

  const artifactButton = document.querySelector('button[aria-label="Selecionar o artefato de Exercícios de fixação"]');
  if (!artifactButton) throw new Error('Não encontrei “Exercícios de fixação” nesta aula.');
  artifactButton.click();
  const dialog = await waitFor(getFixationDialog, 12_000);
  await waitFor(() => {
    try {
      getFixationProgress(dialog);
      return dialog;
    } catch {
      return null;
    }
  }, 20_000);
  return dialog;
}

function getFixationProgress(dialog) {
  const progressElement = [...dialog.querySelectorAll('p, span, div')].find((element) =>
    /^Questão\s+\d+\s+de\s+\d+$/i.test(normalize(element.textContent)),
  );
  const match = normalize(progressElement?.textContent).match(/^Questão\s+(\d+)\s+de\s+(\d+)$/i);
  if (!match) throw new Error('Não encontrei o contador das questões de fixação.');
  return { current: Number(match[1]), total: Number(match[2]) };
}

function waitForFixationProgress(dialog, previousCurrent, timeout = 5_000) {
  return new Promise((resolve, reject) => {
    const check = () => {
      const progress = getFixationProgress(dialog);
      return progress.current !== previousCurrent ? progress : null;
    };
    const immediate = check();
    if (immediate) return resolve(immediate);

    const observer = new MutationObserver(() => {
      const progress = check();
      if (!progress) return;
      clearTimeout(timer);
      observer.disconnect();
      resolve(progress);
    });
    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error('Não consegui avançar para a próxima questão de fixação.'));
    }, timeout);
    observer.observe(dialog, { childList: true, subtree: true, characterData: true });
  });
}

function getFixationQuestion(dialog) {
  const { current } = getFixationProgress(dialog);
  const questionBlock = [...dialog.querySelectorAll('div')].find((element) => {
    const number = normalize(element.querySelector(':scope > span')?.textContent);
    const text = normalize(element.querySelector(':scope > p')?.textContent);
    return number === String(current) && Boolean(text);
  });
  const statement = normalize(questionBlock?.querySelector(':scope > p')?.textContent);
  const options = [...dialog.querySelectorAll('div.cursor-pointer')].map((option) => {
    const label = normalize(option.querySelector('span')?.textContent);
    const text = normalize(option.querySelector('span:last-child')?.textContent);
    return label && text ? `${label}. ${text}` : '';
  }).filter(Boolean);

  if (!statement || options.length < 2) {
    throw new Error(`Não consegui ler a questão ${current} por completo.`);
  }

  return { current, statement, options };
}

function getFixationNavigationButton(dialog, icon) {
  return [...dialog.querySelectorAll('button')].find((button) =>
    button.querySelector(`[data-icon="${icon}"]`) && !button.disabled,
  );
}

async function extractFixationSet() {
  const dialog = getFixationDialog();
  if (!dialog) {
    throw new Error('Abra “Exercícios de Fixação” na aula antes de usar esta opção.');
  }

  const initial = getFixationProgress(dialog);
  const collected = [];

  for (let position = initial.current; position > 1; position -= 1) {
    const previousButton = getFixationNavigationButton(dialog, 'chevron-left');
    if (!previousButton) throw new Error(`Não encontrei o botão para voltar da questão ${position}.`);
    previousButton.click();
    await waitForFixationProgress(dialog, position);
  }

  for (let position = 1; position <= initial.total; position += 1) {
    const question = getFixationQuestion(dialog);
    collected.push(question);
    if (position === initial.total) break;

    const nextButton = getFixationNavigationButton(dialog, 'chevron-right');
    if (!nextButton) throw new Error(`Não encontrei o botão para avançar da questão ${position}.`);
    nextButton.click();
    await waitForFixationProgress(dialog, position);
  }

  for (let position = initial.total; position > initial.current; position -= 1) {
    const previousButton = getFixationNavigationButton(dialog, 'chevron-left');
    if (!previousButton) break;
    previousButton.click();
    await waitForFixationProgress(dialog, position);
  }

  const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
  const text = [
    'EXERCÍCIOS DE FIXAÇÃO',
    `Aula: ${lessonTitle}`,
    `Origem: ${location.href}`,
    `Extraído em: ${new Date().toLocaleString('pt-BR')}`,
    'Gabarito: não informado; a extensão não seleciona nem responde questões.',
    '',
    ...collected.flatMap((question) => [
      `QUESTÃO ${question.current}`,
      question.statement,
      ...question.options,
      '',
    ]),
  ].join('\n');

  return { text: `${text}\n`, title: lessonTitle, questions: collected };
}

function getFlashcardsPanel() {
  // Gran has shipped this player both as a <section> and as a dialog-like
  // overlay. Resolve it from its visible title instead of relying on a single
  // wrapper tag, otherwise the importer sees the button but never sees the
  // player that it just opened.
  return getArtifactPanel(['flashcards']);
}

async function closeArtifactOverlay(panel) {
  const closeButton = [...(panel?.querySelectorAll('button') || [])].find((button) => {
    const label = normalize(`${button.getAttribute('aria-label') || ''} ${button.innerText || button.textContent || ''}`);
    return isVisible(button) && (/^(fechar|close)\b/i.test(label) || /^voltar para (?:a )?aula$/i.test(label));
  });
  if (!closeButton) return;
  closeButton.click();
  // Wait for Gran to unmount its overlay before selecting the next artifact.
  // Its controls may otherwise still be visible in the DOM but blocked by the
  // transition layer.
  await new Promise((resolve) => setTimeout(resolve, 150));
}

async function ensureFlashcardsPanel() {
  let panel = getFlashcardsPanel();
  if (!panel) {
    const artifactButton = findArtifactButton(['flashcards']);
    if (!artifactButton) throw new Error('Não encontrei Flashcards nesta aula.');
    artifactButton.click();
    panel = await waitFor(getFlashcardsPanel, 20_000);
  }

  const tutorialCard = panel.querySelector('div[role="button"][aria-pressed]')
    || panel.querySelector('[role="button"][aria-pressed]')
    || panel.querySelector('[role="button"]');
  const startButton = [...panel.querySelectorAll('button')].find((button) =>
    normalize(button.textContent) === 'Entendi, vamos começar!',
  );
  if (startButton) {
    // The introductory player changed from a div with aria-pressed to a
    // generic role=button in newer Gran lessons. Select it when present, but
    // do not require its former markup in order to start the deck.
    tutorialCard?.click();
    await new Promise((resolve) => setTimeout(resolve, 120));
    startButton.click();
    await waitFor(() => {
      try {
        getFlashcardProgress(panel);
        return panel;
      } catch {
        return null;
      }
    }, 5_000, 'O player de flashcards não iniciou após a tela de introdução.');
  }
  await waitFor(() => {
    panel = getFlashcardsPanel() || panel;
    return getFlashcardProgress(panel) ? panel : null;
  }, 20_000, 'Os flashcards abriram, mas o contador não carregou.');
  return panel;
}

function getFlashcardProgress(panel) {
  const text = normalize(panel?.innerText || panel?.textContent);
  // Gran uses both "1 / 10 cartões" and "Cartão 1 de 10" depending on
  // the lesson/player version.  Prefer an explicit card label, then fall
  // back to the only position/total pair displayed inside the player.
  const labelled = text.match(/(?:flashcards?|cart(?:ão|ões))\s*(\d+)\s*(?:\/|de)\s*(\d+)/i)
    || text.match(/(\d+)\s*(?:\/|de)\s*(\d+)\s*(?:flashcards?|cart(?:ão|ões))/i);
  const match = labelled
    // Some Gran player versions display a bare “1 / 15” in the card panel.
    // It is still unambiguous here because this function only runs inside the
    // Flashcards overlay (never the video player or exercise dialog).
    || [...text.matchAll(/\b(\d+)\s*(?:\/|de)\s*(\d+)\b/gi)]
      .map((candidate) => ({ current: Number(candidate[1]), total: Number(candidate[2]) }))
      .find((candidate) => candidate.current >= 1 && candidate.current <= candidate.total && candidate.total >= 2 && candidate.total <= 100);
  if (!match) throw new Error('Não encontrei o contador dos flashcards.');
  return 'current' in match
    ? match
    : { current: Number(match[1]), total: Number(match[2]) };
}

function getFlashcard(panel) {
  const card = panel.querySelector('div[role="button"][aria-pressed]')
    || panel.querySelector('[role="button"][aria-pressed]');
  const faces = [...(card || panel).querySelectorAll('.backface-hidden, [class*="backface-visibility"], [data-side]')];
  const frontElement = panel.querySelector('[data-side="front"], [data-testid="flashcard-front"]') || faces[0];
  const backElement = panel.querySelector('[data-side="back"], [data-testid="flashcard-back"]') || faces[1];
  const front = normalize(frontElement?.innerText || frontElement?.textContent);
  const back = normalize(backElement?.innerText || backElement?.textContent);
  if (!front || !back) throw new Error('Não consegui ler a frente e o verso deste flashcard.');
  return { ...getFlashcardProgress(panel), front, back };
}

async function extractFlashcards() {
  let panel = null;
  try {
    panel = await ensureFlashcardsPanel();
    const initial = getFlashcardProgress(panel);
    const cards = [];
    for (let position = initial.current; position > 1; position -= 1) {
      const previous = flashcardNavigation(panel, 'previous');
      if (!previous) throw new Error(`Não consegui voltar ao flashcard ${position - 1}.`);
      previous.click();
      await waitFor(() => getFlashcardProgress(panel).current === position - 1 ? panel : null);
    }
    const total = getFlashcardProgress(panel).total;
    for (let position = 1; position <= total; position += 1) {
      const currentCard = await waitForStableArtifact(() => {
        panel = getFlashcardsPanel() || panel;
        try {
          const value = getFlashcard(panel);
          return value.current === position ? value : null;
        } catch { return null; }
      }, 15_000, 400);
      cards.push(currentCard);
      if (position === total) break;
      const next = flashcardNavigation(panel, 'next');
      if (!next) throw new Error(`Não consegui avançar do flashcard ${position}.`);
      next.click();
      await waitFor(() => getFlashcardProgress(panel).current === position + 1 ? panel : null);
    }
    const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
    const text = ['FLASHCARDS', `Aula: ${lessonTitle}`, `Origem: ${location.href}`, `Extraído em: ${new Date().toLocaleString('pt-BR')}`, '', ...cards.flatMap((card) => [`FLASHCARD ${card.current}`, `Frente: ${card.front}`, `Verso: ${card.back}`, ''])].join('\n');
    return { text: `${text}\n`, title: lessonTitle, cards };
  } finally {
    // Do this on failure too, otherwise the following collector is blocked by
    // the player overlay.
    await closeArtifactOverlay(panel || getFlashcardsPanel());
  }
}

function flashcardNavigation(panel, direction) {
  const previous = direction === 'previous';
  return [...panel.querySelectorAll('button')].find((button) => {
    if (!isVisible(button) || button.disabled || button.getAttribute('aria-disabled') === 'true') return false;
    const label = artifactKey(`${button.getAttribute('aria-label') || ''} ${button.getAttribute('title') || ''} ${button.textContent || ''}`);
    return (previous ? /anterior|voltar|previous/ : /proximo|seguinte|next/).test(label)
      || Boolean(button.querySelector(`[data-icon="chevron-${previous ? 'left' : 'right'}"], .lucide-chevron-${previous ? 'left' : 'right'}`));
  });
}

function findArtifactButton(names) {
  const normalizedNames = names.map(artifactKey);
  return [...document.querySelectorAll('button, [role="button"]')].find((button) => {
    if (!isVisible(button) || button.disabled) return false;
    const text = artifactKey(`${button.getAttribute('aria-label') || ''} ${button.getAttribute('title') || ''} ${button.innerText || button.textContent || ''}`);
    if (normalizedNames.includes('resumo') && /resumo e exercicios|resumo de bolso/.test(text)) return false;
    return normalizedNames.some((name) => text.includes(name));
  });
}

function getArtifactPanel(names) {
  const normalizedNames = names.map(artifactKey);
  const titles = [...document.querySelectorAll('#player-overlay-title, h1, h2, h3, [role="heading"]')];
  for (const title of titles) {
    if (!isVisible(title)) continue;
    const text = artifactKey(title.textContent);
    if (!normalizedNames.some((name) => text === name || text.startsWith(`${name} -`) || text.startsWith(`${name} —`))) continue;
    const panel = title.closest('[role="dialog"], dialog, .drawer-artifacts-drawer, .mindmap-overlay, section, [class*="overlay"], [class*="Overlay"]');
    if (panel && isVisible(panel)) return panel;
  }
  return null;
}

function artifactPanelText(panel) {
  const ignored = /^(fechar|voltar para a aula|próximo|anterior|entendi, vamos começar!)$/i;
  const chunks = [...panel.querySelectorAll('h1, h2, h3, h4, h5, h6, p, li')]
    .filter((element) => !element.closest('button, [aria-busy="true"], [hidden], [aria-hidden="true"]')
      && !(element.tagName === 'P' && element.closest('li'))
      && element.id !== 'player-overlay-title' && isVisible(element))
    .map((element) => {
      const text = normalize(element.innerText || element.textContent);
      if (/^H[1-6]$/.test(element.tagName)) return `${'#'.repeat(Math.min(6, Number(element.tagName.slice(1)) + 1))} ${inlineMarkdown(element)}`;
      return element.tagName === 'LI' ? `- ${inlineMarkdown(element)}` : inlineMarkdown(element);
    })
    .filter((text) => text && !ignored.test(text));
  // Repeated list items may be meaningful; do not globally deduplicate them.
  return chunks.join('\n\n').trim();
}

async function extractTextArtifact(names) {
  const button = findArtifactButton(names);
  if (!button) throw new Error('não disponível nesta aula');
  button.click();
  try {
    await waitFor(
      () => getArtifactPanel(names),
      8_000,
      `O material “${names[0]}” demorou para abrir. Tente novamente.`,
    );
    return await waitForStableArtifact(() => {
      const currentPanel = getArtifactPanel(names);
      if (!currentPanel || currentPanel.querySelector('[aria-busy="true"]')) return null;
      const text = artifactPanelText(currentPanel);
      const body = text.split('\n').filter((line) => !line.startsWith('#')).join(' ').trim();
      if (/^(?:nao foi possivel|ocorreu um erro|conteudo indisponivel|material indisponivel)/.test(artifactKey(body))) {
        throw new Error(`O Gran exibiu uma falha ao carregar ${names[0]}.`);
      }
      if (/carregando|gerando (?:resumo|revisao)|aguarde/i.test(body) || body.length < 40) return null;
      return text;
    });
  } finally {
    await closeArtifactOverlay(getArtifactPanel(names));
  }
}

async function extractMindMaps() {
  const names = ['mapa mental', 'mapas mentais'];
  const button = findArtifactButton(names);
  if (!button) throw new Error('não disponível nesta aula');
  button.click();
  try {
    let panel = await waitFor(
      () => getArtifactPanel(names),
      8_000,
      'O mapa mental demorou para abrir. Tente novamente.',
    );
    // Wait for actual map content, not its drawer shell or an error graphic.
    await waitForStableArtifact(() => {
      const current = getArtifactPanel(names);
      const svg = current?.querySelector('svg.markmap');
      if (svg?.querySelector('g.markmap-node') && normalize(svg.textContent)) return svg.outerHTML;
      return getMindMapImages(current).join('\n') || null;
    });
    panel = getArtifactPanel(names) || panel;

    // Gran uses Markmap. Its initial view renders only root branches; open all
    // nodes before serialising so StudyFlash receives the complete structure.
    const expandAll = [...panel.querySelectorAll('button')].find((candidate) =>
      normalize(candidate.innerText || candidate.textContent) === 'Abrir tudo',
    );
    const firstMap = panel.querySelector('svg.markmap');
    const before = firstMap?.querySelectorAll('g.markmap-node').length || 0;
    if (expandAll && firstMap) {
      expandAll.click();
      const startedAt = Date.now();
      let lastCount = before;
      let stableSince = Date.now();
      while (Date.now() - startedAt < 4_000) {
        await new Promise((resolve) => setTimeout(resolve, 120));
        const count = firstMap.querySelectorAll('g.markmap-node').length;
        if (count !== lastCount) {
          lastCount = count;
          stableSince = Date.now();
        }
        if (count > before && Date.now() - stableSince > 500) break;
      }
      await new Promise((resolve) => setTimeout(resolve, 300));
    }

    // Keep Markmap's data-path attributes: the StudyFlash viewer uses them to
    // collapse and expand descendant topics.
    const maps = [...panel.querySelectorAll('svg.markmap')].map((svg) => {
      const copy = svg.cloneNode(true);
      copy.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      const markup = new XMLSerializer().serializeToString(copy);
      return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(markup)}`;
    });
    if (maps.length) return maps;
    const images = getMindMapImages(panel);
    if (!images.length) throw new Error('O mapa mental abriu, mas não continha uma imagem ou mapa vetorial legível.');
    return images;
  } finally {
    await closeArtifactOverlay(getArtifactPanel(names));
  }
}

function getMindMapImages(panel) {
  if (!panel) return [];
  return [...panel.querySelectorAll('img')].filter((image) => {
    const description = artifactKey(`${image.alt || ''} ${image.currentSrc || image.src || ''}`);
    if (/error|erro|empty|placeholder|not.found|nao.encontrad|illustration|ilustracao|loading|spinner|logo|icon/.test(description)) return false;
    return /mapa|mind.?map/.test(description) && image.complete !== false
      && (image.naturalWidth || Number(image.getAttribute('width')) || 0) > 200;
  }).map((image) => image.currentSrc || image.src).filter((source) => /^https?:\/\//.test(source));
}

function lessonMetadata() {
  const title = normalize(document.querySelector('main h2')?.textContent) || 'Aula importada do Gran';
  const extractLabeledValue = (label) => {
    const element = [...document.querySelectorAll('p, span, div')].find((candidate) =>
      normalize(candidate.textContent) === label,
    );
    const sibling = element?.nextElementSibling;
    return normalize(sibling?.textContent) || '';
  };
  return {
    title,
    source_url: location.href,
    discipline: extractLabeledValue('DISCIPLINA'),
    topic: extractLabeledValue('TÓPICO'),
  };
}

function fixationQuestionsAsCards(questions, metadata, timezoneOffsetMinutes) {
  return questions
    .map((question) => {
      const alternatives = Array.isArray(question.alternatives) ? question.alternatives : [];
      const correct = alternatives.find((alternative) => alternative.correct)
        || alternatives.find((alternative) => alternative.letter === question.correct);
      const choices = alternatives.map((alternative) => normalize(alternative.text)).filter(Boolean);
      if (!question.statement || !correct?.text || choices.length < 2) return null;

      const explanation = [
        `Gabarito: ${correct.letter || question.correct}. ${correct.explanation || ''}`.trim(),
        ...alternatives
          .filter((alternative) => !alternative.correct && alternative.explanation)
          .map((alternative) => `${alternative.letter}) ${alternative.explanation}`),
      ].filter(Boolean).join('\n\n');
      return {
        card_type: 'multiple_choice',
        front: question.statement,
        back: correct.text,
        explanation,
        choices,
        correct_answer: correct.text,
        tags: ['gran', 'questão de fixação', metadata.discipline, metadata.topic].filter(Boolean),
        subject_id: null,
        topic_id: null,
        content_id: null,
        source_view_id: null,
        source_block_id: null,
        timezone_offset_minutes: timezoneOffsetMinutes,
        initial_difficulty: null,
      };
    })
    .filter(Boolean);
}

async function collectLessonPackage() {
  const metadata = lessonMetadata();
  const timezoneOffsetMinutes = -new Date().getTimezoneOffset();
  const skipped = [];
  const optional = async (label, collector, fallback) => {
    try {
      return await collector();
    } catch (error) {
      const reason = error?.message || String(error);
      console.info(`StudyFlash skipped ${label}:`, reason);
      skipped.push(`${label}: ${reason}`);
      // Collectors are independent. Clear any incomplete Gran modal before
      // attempting the next one, so one unavailable artifact cannot cascade
      // into misleading errors for the others.
      await closeArtifactOverlay(getTranscriptDialog());
      await closeArtifactOverlay(getFlashcardsPanel());
      await closeArtifactOverlay(getFixationDialog());
      return fallback;
    }
  };

  const transcript = await optional('Transcrição', extractTranscript, { transcript: '' });
  const summary = await optional('Resumo', () => extractTextArtifact(['resumo da aula', 'resumo de aula', 'resumo']), '');
  const pocketReview = await optional('Revisão de bolso', () => extractTextArtifact(['revisão de bolso', 'resumo de bolso']), '');
  const flashcards = await optional('Flashcards', extractFlashcards, { cards: [] });
  const questions = await optional('Exercícios de fixação', answerAndExtractFixationWithExplanations, { questions: [] });
  const mindMaps = await optional('Mapa mental', extractMindMaps, []);

  return { package: {
    ...metadata,
    transcript: transcript.transcript || '',
    summary,
    pocket_review: pocketReview,
    questions: questions.questions || [],
    mind_maps: mindMaps,
    cards: [
      ...(flashcards.cards || []).map((card) => ({
        card_type: 'classic',
        front: card.front,
        back: card.back,
        explanation: '',
        choices: [],
        correct_answer: '',
        tags: ['gran', metadata.discipline, metadata.topic].filter(Boolean),
        subject_id: null,
        topic_id: null,
        content_id: null,
        source_view_id: null,
        source_block_id: null,
        timezone_offset_minutes: timezoneOffsetMinutes,
        initial_difficulty: null,
      })),
      ...fixationQuestionsAsCards(questions.questions || [], metadata, timezoneOffsetMinutes),
    ],
    subject_id: null,
    topic_id: null,
    content_id: null,
    timezone_offset_minutes: timezoneOffsetMinutes,
    skipped_materials: skipped,
  } };
}

function getFixationAnswer(dialog) {
  const plainText = normalize(dialog.innerText || '');
  const explicit = plainText.match(/(?:gabarito|resposta correta|alternativa correta)\s*:?\s*([A-E])\b/i);
  if (explicit) return explicit[1].toUpperCase();

  const correctOption = [...dialog.querySelectorAll('div.cursor-default, div.cursor-pointer')].find((option) =>
    normalize(option.innerText).includes('OPÇÃO CORRETA'),
  );
  if (correctOption) {
    return [...correctOption.querySelectorAll('span')]
      .map((element) => normalize(element.textContent))
      .find((text) => /^[A-E]$/.test(text)) || null;
  }

  const classMatchedOption = [...dialog.querySelectorAll('div.cursor-default, div.cursor-pointer')].find((option) => {
    const classes = [option, ...option.querySelectorAll('*')]
      .map((element) => element.className?.toString() || '')
      .join(' ');
    return /correct|success|emerald|green|acerto|right/i.test(classes);
  });
  return [...(classMatchedOption?.querySelectorAll('span') || [])]
    .map((element) => normalize(element.textContent))
    .find((text) => /^[A-E]$/.test(text)) || null;
}

function waitForFixationAnswer(dialog, timeout = 8_000) {
  return new Promise((resolve, reject) => {
    const immediately = getFixationAnswer(dialog);
    if (immediately) return resolve(immediately);

    const observer = new MutationObserver(() => {
      const answer = getFixationAnswer(dialog);
      if (!answer) return;
      clearTimeout(timer);
      observer.disconnect();
      resolve(answer);
    });
    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error('O Gran não exibiu um gabarito que a extensão conseguiu reconhecer.'));
    }, timeout);
    observer.observe(dialog, { childList: true, subtree: true, characterData: true, attributes: true });
  });
}

function waitForEnabledAnswerButton(dialog, timeout = 5_000) {
  return new Promise((resolve, reject) => {
    const findButton = () => [...dialog.querySelectorAll('button')].find((button) =>
      normalize(button.textContent) === 'Responder' && !button.disabled,
    );
    const immediately = findButton();
    if (immediately) return resolve(immediately);

    const observer = new MutationObserver(() => {
      const button = findButton();
      if (!button) return;
      clearTimeout(timer);
      observer.disconnect();
      resolve(button);
    });
    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error('O botão “Responder” não foi habilitado após selecionar a alternativa.'));
    }, timeout);
    observer.observe(dialog, { childList: true, subtree: true, attributes: true, characterData: true });
  });
}

async function answerAndExtractFixationSet() {
  const dialog = getFixationDialog();
  if (!dialog) throw new Error('Abra “Exercícios de Fixação” na aula antes de usar esta opção.');

  const initial = getFixationProgress(dialog);
  const collected = [];

  for (let position = initial.current; position > 1; position -= 1) {
    const previousButton = getFixationNavigationButton(dialog, 'chevron-left');
    if (!previousButton) throw new Error(`Não encontrei o botão para voltar da questão ${position}.`);
    previousButton.click();
    await waitForFixationProgress(dialog, position);
  }

  const total = getFixationProgress(dialog).total;
  for (let position = 1; position <= total; position += 1) {
    const question = getFixationQuestion(dialog);
    const firstOption = dialog.querySelector('div.cursor-pointer');
    if (!firstOption) throw new Error(`Não encontrei uma alternativa para a questão ${position}.`);

    firstOption.click();
    const answerButton = await waitForEnabledAnswerButton(dialog);

    answerButton.click();
    const answer = await waitForFixationAnswer(dialog);
    collected.push({ ...question, answer });

    if (position === total) break;
    const nextButton = getFixationNavigationButton(dialog, 'chevron-right');
    if (!nextButton) throw new Error(`Não encontrei o botão para avançar da questão ${position}.`);
    nextButton.click();
    await waitForFixationProgress(dialog, position);
  }

  for (let position = total; position > initial.current; position -= 1) {
    const previousButton = getFixationNavigationButton(dialog, 'chevron-left');
    if (!previousButton) break;
    previousButton.click();
    await waitForFixationProgress(dialog, position);
  }

  const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
  const text = [
    'EXERCÍCIOS DE FIXAÇÃO',
    `Aula: ${lessonTitle}`,
    `Origem: ${location.href}`,
    `Extraído em: ${new Date().toLocaleString('pt-BR')}`,
    'Observação: a extensão marcou a alternativa A para revelar os gabaritos no Gran.',
    '',
    ...collected.flatMap((question) => [
      `QUESTÃO ${question.current}`,
      question.statement,
      ...question.options,
      '',
    ]),
    'GABARITOS',
    ...collected.map((question) => `${question.current}. ${question.answer}`),
  ].join('\n');

  return { text: `${text}\n`, title: lessonTitle, questions: collected };
}

async function getReviewedOptions(dialog, question) {
  const expected = question.options.map((option) => {
    const match = option.match(/^([A-E])\.\s*(.*)$/);
    return { letter: match?.[1], text: match?.[2] };
  });

  // O comentário fica como irmão do bloco da alternativa; por isso lemos o texto
  // completo do diálogo, e não apenas o elemento com a classe cursor-default.
  for (const toggle of [...dialog.querySelectorAll('button')].filter((button) =>
    button.querySelector('[data-icon="chevron-down"]'),
  )) {
    toggle.click();
    await new Promise((resolve) => setTimeout(resolve, 80));
  }

  const read = () => {
    const fullText = dialog.innerText || '';
    const starts = expected.map(({ letter, text }) => fullText.indexOf(`${letter}\n${text}`));
    if (starts.some((start) => start < 0)) return null;
    const alternatives = expected.map(({ letter, text }, index) => {
      const afterText = starts[index] + `${letter}\n${text}`.length;
      const nextStart = starts.slice(index + 1).find((start) => start > afterText);
      const end = nextStart ?? fullText.indexOf('\nResponder', afterText);
      const segment = fullText.slice(afterText, end < 0 ? undefined : end);
      return {
        letter,
        text,
        correct: segment.includes('OPÇÃO CORRETA'),
        explanation: normalize(segment.replace('OPÇÃO CORRETA', '')),
      };
    });
    return alternatives.every((alternative) => alternative.explanation) ? alternatives : null;
  };

  const immediate = read();
  if (immediate) return immediate;
  return waitFor(read, 5_000);
}

async function answerAndExtractFixationWithExplanations() {
  const dialog = await ensureFixationDialog();

  const initial = getFixationProgress(dialog);
  const collected = [];
  for (let position = initial.current; position > 1; position -= 1) {
    const previousButton = getFixationNavigationButton(dialog, 'chevron-left');
    if (!previousButton) throw new Error(`Não encontrei o botão para voltar da questão ${position}.`);
    previousButton.click();
    await waitForFixationProgress(dialog, position);
  }

  const total = getFixationProgress(dialog).total;
  for (let position = 1; position <= total; position += 1) {
    const question = getFixationQuestion(dialog);
    const firstOption = dialog.querySelector('div.cursor-pointer');
    if (!firstOption) throw new Error(`Não encontrei uma alternativa para a questão ${position}.`);
    firstOption.click();
    const answerButton = await waitForEnabledAnswerButton(dialog);
    answerButton.click();
    await waitForFixationAnswer(dialog);

    const alternatives = await getReviewedOptions(dialog, question);
    const correct = alternatives.find((alternative) => alternative.correct)?.letter;
    if (!correct || alternatives.some((alternative) => !alternative.explanation)) throw new Error(`A explicação da questão ${position} não foi carregada por completo.`);
    collected.push({ ...question, correct, alternatives });

    if (position === total) break;
    const nextButton = [...dialog.querySelectorAll('button')].find((button) => normalize(button.textContent) === 'Próxima pergunta');
    if (!nextButton) throw new Error(`Não encontrei o botão para avançar da questão ${position}.`);
    nextButton.click();
    await waitForFixationProgress(dialog, position);
  }

  const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
  const text = ['EXERCÍCIOS DE FIXAÇÃO COM GABARITOS E EXPLICAÇÕES', `Aula: ${lessonTitle}`, `Origem: ${location.href}`, `Extraído em: ${new Date().toLocaleString('pt-BR')}`, 'Observação: a extensão marcou a alternativa A para liberar as correções no Gran.', '', ...collected.flatMap((question) => [
    `QUESTÃO ${question.current}`,
    `Enunciado: ${question.statement}`,
    `GABARITO: ${question.correct}`,
    '',
    ...question.alternatives.flatMap((alternative) => [
      `${alternative.letter}) ${alternative.text}`,
      `${alternative.correct ? 'CORRETA' : 'INCORRETA'}: ${alternative.explanation}`,
      '',
    ]),
  ])].join('\n');
  // Do not leave the exercise modal covering the controls used by the next
  // collection step (for example, the mental-map artifact).
  await closeArtifactOverlay(dialog);
  return { text: `${text}\n`, title: lessonTitle, questions: collected };
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const extractor = message?.type === 'EXTRACT_TRANSCRIPT'
    ? extractTranscript
    : message?.type === 'EXTRACT_FIXATION_QUESTION'
      ? extractFixationQuestion
      : message?.type === 'EXTRACT_FIXATION_SET'
        ? extractFixationSet
        : message?.type === 'EXTRACT_FLASHCARDS'
          ? extractFlashcards
        : message?.type === 'COLLECT_LESSON_PACKAGE'
          ? collectLessonPackage
        : message?.type === 'ANSWER_AND_EXTRACT_FIXATION_SET'
          ? answerAndExtractFixationSet
        : message?.type === 'ANSWER_AND_EXTRACT_FIXATION_WITH_EXPLANATIONS'
          ? answerAndExtractFixationWithExplanations
        : null;

  if (!extractor) return;

  Promise.resolve()
    .then(extractor)
    .then((result) => sendResponse({ ok: true, ...result }))
    .catch((error) => sendResponse({ ok: false, error: error.message || String(error) }));

  return true;
});
