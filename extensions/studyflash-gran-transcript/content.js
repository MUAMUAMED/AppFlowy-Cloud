// Gran occasionally removes a node while an overlay is being rendered.  In
// that short interval textContent/innerText can be null (not just undefined),
// so always coerce before normalising it.
const normalize = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();

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

function waitFor(getElement, timeout = 8_000, timeoutMessage = 'O conteúdo demorou para abrir. Tente novamente.') {
  return new Promise((resolve, reject) => {
    const immediately = getElement();
    if (immediately) return resolve(immediately);

    const observer = new MutationObserver(() => {
      const element = getElement();
      if (!element) return;
      clearTimeout(timer);
      observer.disconnect();
      resolve(element);
    });

    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error(timeoutMessage));
    }, timeout);

    observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
  });
}

function getTranscriptDialog() {
  return [...document.querySelectorAll('[role="dialog"], dialog')].find((dialog) => {
    const text = normalize(dialog.innerText || dialog.textContent);
    return text.startsWith('Transcrição');
  });
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

  for (const node of contentNodes) {
    const text = normalize(node.innerText || node.textContent);
    if (!text || text === 'Transcrição' || text === 'Expandir' || text === 'Fechar' || seen.has(text)) continue;
    seen.add(text);

    if (/^H[1-4]$/.test(node.tagName)) lines.push(`\n${text.toUpperCase()}\n`);
    else if (node.tagName === 'LI') lines.push(`- ${text}`);
    else lines.push(text);
  }

  if (lines.length <= 4) {
    const fallback = normalize(dialog.innerText || dialog.textContent)
      .replace(/^Transcrição\s*(Expandir)?\s*(Fechar)?\s*/i, '');
    if (fallback) lines.push(fallback);
  }

  return lines.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
}

async function extractTranscript() {
  const lessonTitle = normalize(document.querySelector('main h2')?.textContent) || 'aula-gran';
  let dialog = null;
  try {
    const summaryButton = await waitFor(findSummaryButton, 8_000);

    summaryButton.click();
    const menuItem = await waitFor(findTranscriptMenuItem);
    menuItem.click();

    await waitFor(getTranscriptDialog, 12_000);
    dialog = await waitFor(getReadyTranscriptDialog, 30_000);
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
  return [...document.querySelectorAll('section')].find((section) =>
    normalize(section.querySelector('[id="player-overlay-title"]')?.textContent) === 'Flashcards',
  );
}

async function closeArtifactOverlay(panel) {
  const closeButton = [...(panel?.querySelectorAll('button') || [])].find((button) => {
    const label = normalize(`${button.getAttribute('aria-label') || ''} ${button.innerText || button.textContent || ''}`);
    return /^(fechar|voltar para a aula)$/i.test(label);
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
    const artifactButton = document.querySelector('button[aria-label="Selecionar o artefato de Flashcards"]');
    if (!artifactButton) throw new Error('Não encontrei Flashcards nesta aula.');
    artifactButton.click();
    panel = await waitFor(getFlashcardsPanel, 12_000);
  }

  const tutorialCard = panel.querySelector('div[role="button"][aria-pressed]');
  const startButton = [...panel.querySelectorAll('button')].find((button) =>
    normalize(button.textContent) === 'Entendi, vamos começar!',
  );
  if (tutorialCard && startButton) {
    tutorialCard.click();
    await new Promise((resolve) => setTimeout(resolve, 120));
    startButton.click();
    await waitFor(() => panel.querySelector('div[role="button"][aria-pressed]')?.querySelectorAll('.backface-hidden').length >= 2 ? panel : null, 5_000);
  }
  return panel;
}

function getFlashcardProgress(panel) {
  const text = normalize(panel?.innerText || panel?.textContent);
  // Gran uses both "1 / 10 cartões" and "Cartão 1 de 10" depending on
  // the lesson/player version.  Prefer an explicit card label, then fall
  // back to the only position/total pair displayed inside the player.
  const match = text.match(/(?:flashcards?|cart(?:ão|ões))\s*(\d+)\s*(?:\/|de)\s*(\d+)/i)
    || text.match(/(\d+)\s*(?:\/|de)\s*(\d+)\s*(?:flashcards?|cart(?:ão|ões))?/i);
  if (!match) throw new Error('Não encontrei o contador dos flashcards.');
  return { current: Number(match[1]), total: Number(match[2]) };
}

function getFlashcard(panel) {
  const card = panel.querySelector('div[role="button"][aria-pressed]');
  const faces = card ? [...card.querySelectorAll('.backface-hidden')] : [];
  const front = normalize(faces[0]?.querySelector('p')?.textContent);
  const back = normalize(faces[1]?.querySelector('p')?.textContent);
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
      const previous = [...panel.querySelectorAll('button')].find((button) => button.querySelector('[data-icon="chevron-left"]') && !button.disabled);
      if (!previous) throw new Error(`Não consegui voltar ao flashcard ${position - 1}.`);
      previous.click();
      await waitFor(() => getFlashcardProgress(panel).current === position - 1 ? panel : null);
    }
    const total = getFlashcardProgress(panel).total;
    for (let position = 1; position <= total; position += 1) {
      cards.push(getFlashcard(panel));
      if (position === total) break;
      const next = [...panel.querySelectorAll('button')].find((button) => button.querySelector('[data-icon="chevron-right"]') && !button.disabled);
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

function findArtifactButton(names) {
  const normalizedNames = names.map((name) => normalize(name).toLowerCase());
  return [...document.querySelectorAll('button')].find((button) => {
    const text = normalize(`${button.getAttribute('aria-label') || ''} ${button.innerText || ''}`).toLowerCase();
    return normalizedNames.some((name) => text.includes(name));
  });
}

function getArtifactPanel(names) {
  const normalizedNames = names.map((name) => normalize(name).toLowerCase());
  return [...document.querySelectorAll('section, [role="dialog"], dialog')].find((panel) => {
    const title = normalize(panel.querySelector('#player-overlay-title, h1, h2, h3')?.textContent).toLowerCase();
    return normalizedNames.some((name) => title.includes(name));
  });
}

function artifactPanelText(panel) {
  const ignored = /^(fechar|voltar para a aula|próximo|anterior|entendi, vamos começar!)$/i;
  const chunks = [...panel.querySelectorAll('h1, h2, h3, h4, p, li')]
    .map((element) => {
      const text = normalize(element.innerText || element.textContent);
      return element.tagName === 'LI' ? `- ${text}` : text;
    })
    .filter((text) => text && !ignored.test(text));
  return [...new Set(chunks)].join('\n\n').trim();
}

async function extractTextArtifact(names) {
  const button = findArtifactButton(names);
  if (!button) throw new Error('não disponível nesta aula');
  button.click();
  try {
    const panel = await waitFor(
      () => getArtifactPanel(names),
      8_000,
      `O material “${names[0]}” demorou para abrir. Tente novamente.`,
    );
    await new Promise((resolve) => setTimeout(resolve, 300));
    return artifactPanelText(panel);
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
    const panel = await waitFor(
      () => getArtifactPanel(names),
      8_000,
      'O mapa mental demorou para abrir. Tente novamente.',
    );
    await new Promise((resolve) => setTimeout(resolve, 300));
    return [...panel.querySelectorAll('img')]
      .map((image) => image.currentSrc || image.src)
      .filter((source) => /^https?:\/\//.test(source));
  } finally {
    await closeArtifactOverlay(getArtifactPanel(names));
  }
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
  const summary = await optional('Resumo', () => extractTextArtifact(['resumo da aula', 'resumo de aula']), '');
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
