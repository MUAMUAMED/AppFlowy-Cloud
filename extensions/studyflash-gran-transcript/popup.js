const STUDYFLASH_ORIGIN = 'https://studyflash-revisao-6a5a42.zeabur.app';
const GRAN_LESSON_URL = /^https:\/\/(www\.)?grancursosonline\.com\.br\/aluno\/curso\/video\//;
const STUDYFLASH_URL = /^https:\/\/studyflash-revisao-6a5a42\.zeabur\.app\//;

const extractButton = document.querySelector('#extract');
const extractQuestionButton = document.querySelector('#extract-question');
const extractFixationButton = document.querySelector('#extract-fixation');
const extractFlashcardsButton = document.querySelector('#extract-flashcards');
const extractFixationFullButton = document.querySelector('#extract-fixation-full');
const connectButton = document.querySelector('#connect-studyflash');
const transferButton = document.querySelector('#transfer-all');
const destination = document.querySelector('#destination');
const status = document.querySelector('#status');

function setStatus(message, isError = false) {
  status.textContent = message;
  status.style.color = isError ? '#b91c1c' : '#526078';
}

function safeFilename(value) {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-zA-Z0-9._-]+/g, '-')
    .replace(/(^-+|-+$)/g, '')
    .slice(0, 90) || 'transcricao-gran';
}

function authorization(token) {
  return { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
}

async function api(path, token, options = {}) {
  const response = await fetch(`${STUDYFLASH_ORIGIN}${path}`, {
    ...options,
    headers: { ...authorization(token), ...(options.headers || {}) },
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok || body.code !== 0) {
    throw new Error(body.message || `StudyFlash respondeu com erro ${response.status}.`);
  }
  return body.data;
}

function flattenFolder(view, level = 0, result = []) {
  if (!view?.view_id) return result;
  result.push({ id: view.view_id, name: view.name || 'Sem nome', level });
  for (const child of view.children || []) flattenFolder(child, level + 1, result);
  return result;
}

function renderDestinations(folder, selectedId, taxonomy = []) {
  // The workspace root itself is not a visible document container.  Only its
  // spaces and their child pages may receive the imported lesson.
  const entries = flattenFolder(folder).slice(1).filter((entry) => entry.id);
  const kindByViewId = new Map(
    taxonomy.filter((node) => node.source_view_id).map((node) => [node.source_view_id, node.kind]),
  );
  const kindLabel = { subject: 'Matéria', topic: 'Tópico', content: 'Conteúdo' };
  destination.replaceChildren();
  for (const entry of entries) {
    const option = document.createElement('option');
    option.value = entry.id;
    const kind = kindByViewId.get(entry.id);
    option.textContent = `${'— '.repeat(entry.level)}${kind ? `[${kindLabel[kind]}] ` : ''}${entry.name}`;
    option.selected = entry.id === selectedId;
    destination.append(option);
  }
  destination.disabled = entries.length === 0;
  transferButton.disabled = entries.length === 0;
}

async function loadSavedConnection() {
  const { studyflashConnection } = await chrome.storage.session.get('studyflashConnection');
  if (!studyflashConnection?.token || !studyflashConnection?.workspaceId) return null;
  try {
    const [folder, taxonomy] = await Promise.all([
      api(`/api/workspace/${studyflashConnection.workspaceId}/folder?depth=10`, studyflashConnection.token),
      api(`/api/review/${studyflashConnection.workspaceId}/taxonomy`, studyflashConnection.token),
    ]);
    renderDestinations(folder, studyflashConnection.parentViewId, taxonomy);
    connectButton.textContent = 'StudyFlash conectado';
    return { ...studyflashConnection, folder };
  } catch {
    await chrome.storage.session.remove('studyflashConnection');
    return null;
  }
}

async function readStudyFlashSession(tabId) {
  const [result] = await chrome.scripting.executeScript({
    target: { tabId },
    world: 'MAIN',
    func: () => {
      try {
        const saved = JSON.parse(localStorage.getItem('token') || 'null');
        return saved?.access_token || null;
      } catch {
        return null;
      }
    },
  });
  return result?.result || null;
}

async function connectStudyFlash() {
  connectButton.disabled = true;
  setStatus('Procurando uma sessão aberta do StudyFlash…');
  try {
    const tabs = await chrome.tabs.query({});
    const tab = tabs.find((candidate) => STUDYFLASH_URL.test(candidate.url || ''));
    if (!tab?.id) throw new Error('Abra o StudyFlash no navegador e entre na sua conta antes de conectar.');
    const token = await readStudyFlashSession(tab.id);
    if (!token) throw new Error('Não encontrei uma sessão válida nessa aba do StudyFlash. Entre novamente e tente conectar.');

    const profile = await api('/api/user/workspace', token);
    const workspace = profile.visiting_workspace || profile.selected_workspace || profile.workspaces?.[0];
    if (!workspace?.workspace_id) throw new Error('Nenhum espaço de trabalho foi encontrado na sua conta.');
    const [folder, taxonomy] = await Promise.all([
      api(`/api/workspace/${workspace.workspace_id}/folder?depth=10`, token),
      api(`/api/review/${workspace.workspace_id}/taxonomy`, token),
    ]);
    const connection = { token, workspaceId: workspace.workspace_id, parentViewId: '' };
    await chrome.storage.session.set({ studyflashConnection: connection });
    renderDestinations(folder, connection.parentViewId, taxonomy);
    connectButton.textContent = 'StudyFlash conectado';
    setStatus('Conectado. Agora escolha onde a aula deve ser criada.');
  } catch (error) {
    setStatus(error.message || 'Não foi possível conectar ao StudyFlash.', true);
  } finally {
    connectButton.disabled = false;
  }
}

async function getConnection() {
  const { studyflashConnection } = await chrome.storage.session.get('studyflashConnection');
  if (!studyflashConnection?.token || !studyflashConnection?.workspaceId) throw new Error('Conecte ao StudyFlash primeiro.');
  return { ...studyflashConnection, parentViewId: destination.value || studyflashConnection.parentViewId };
}

// A content script registered in the manifest only starts automatically on a
// new navigation.  If the user reloads the extension while a Gran lesson is
// already open, Chrome has no receiver in that tab yet.  Recover in place so
// the user never has to refresh the lesson just to import it.
async function sendGranMessage(tabId, message) {
  try {
    return await chrome.tabs.sendMessage(tabId, message);
  } catch (error) {
    const details = error?.message || String(error);
    if (!/receiving end does not exist|could not establish connection/i.test(details)) throw error;
    await chrome.scripting.executeScript({ target: { tabId }, files: ['content.js'] });
    return chrome.tabs.sendMessage(tabId, message);
  }
}

async function sendToStudyFlash() {
  transferButton.disabled = true;
  setStatus('Coletando os materiais disponíveis da aula… isso pode levar alguns minutos.');
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id || !GRAN_LESSON_URL.test(tab.url || '')) throw new Error('Abra uma aula do Gran antes de transferir.');
    const confirmed = window.confirm(
      'Para capturar gabaritos e explicações, a extensão marcará a alternativa A em cada questão. Isso será registrado no Gran. Deseja transferir a revisão completa?',
    );
    if (!confirmed) return;

    const result = await sendGranMessage(tab.id, { type: 'COLLECT_LESSON_PACKAGE' });
    if (!result?.ok) throw new Error(result?.error || 'Não consegui coletar os materiais da aula.');
    const connection = await getConnection();
    const response = await api(`/api/review/${connection.workspaceId}/lesson-import`, connection.token, {
      method: 'POST',
      body: JSON.stringify({ ...result.package, parent_view_id: connection.parentViewId }),
    });
    await chrome.storage.session.set({ studyflashConnection: connection });
    const skipped = result.package.skipped_materials || [];
    if (skipped.length) {
      setStatus(`Importação concluída com ${skipped.length} material(is) não disponível(is): ${skipped.map((item) => item.split(':')[0]).join(', ')}. Página criada e ${response.imported_cards} flashcard(s) adicionados.`);
    } else {
      setStatus(`Importação concluída: página criada e ${response.imported_cards} flashcard(s) adicionados à Revisão.`);
    }
  } catch (error) {
    setStatus(error.message || 'Não foi possível transferir a revisão.', true);
  } finally {
    transferButton.disabled = false;
  }
}

async function extractToText({ button, messageType, allowedUrl, filenameSuffix, loadingText }) {
  button.disabled = true;
  setStatus(loadingText);
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id || !allowedUrl.test(tab.url || '')) throw new Error('Abra a página correta do Gran antes de usar esta opção.');
    const result = await sendGranMessage(tab.id, { type: messageType });
    if (!result?.ok) throw new Error(result?.error || 'Não foi possível extrair o conteúdo.');
    const blobUrl = URL.createObjectURL(new Blob([result.text], { type: 'text/plain;charset=utf-8' }));
    await chrome.downloads.download({ url: blobUrl, filename: `studyflash/gran/${safeFilename(result.title)}-${filenameSuffix}.txt`, saveAs: true });
    setStatus('Pronto. Escolha onde salvar o TXT.');
    setTimeout(() => URL.revokeObjectURL(blobUrl), 30_000);
  } catch (error) {
    setStatus(error.message || 'Ocorreu um erro inesperado.', true);
  } finally {
    button.disabled = false;
  }
}

connectButton.addEventListener('click', () => void connectStudyFlash());
transferButton.addEventListener('click', () => void sendToStudyFlash());
extractButton.addEventListener('click', () => extractToText({ button: extractButton, messageType: 'EXTRACT_TRANSCRIPT', allowedUrl: GRAN_LESSON_URL, filenameSuffix: 'transcricao', loadingText: 'Abrindo e lendo a transcrição…' }));
extractQuestionButton.addEventListener('click', () => extractToText({ button: extractQuestionButton, messageType: 'EXTRACT_FIXATION_QUESTION', allowedUrl: /^https:\/\/questoes\.grancursosonline\.com\.br\/questoes-de-concursos\//, filenameSuffix: 'questao', loadingText: 'Lendo enunciado e alternativas…' }));
extractFixationButton.addEventListener('click', () => extractToText({ button: extractFixationButton, messageType: 'EXTRACT_FIXATION_SET', allowedUrl: GRAN_LESSON_URL, filenameSuffix: 'exercicios-fixacao', loadingText: 'Lendo as questões de fixação…' }));
extractFlashcardsButton.addEventListener('click', () => extractToText({ button: extractFlashcardsButton, messageType: 'EXTRACT_FLASHCARDS', allowedUrl: GRAN_LESSON_URL, filenameSuffix: 'flashcards', loadingText: 'Abrindo e lendo os flashcards…' }));
extractFixationFullButton.addEventListener('click', () => {
  if (!window.confirm('A extensão marcará a alternativa A e responderá todas as questões para obter explicações. Isso ficará registrado no Gran. Deseja continuar?')) return;
  void extractToText({ button: extractFixationFullButton, messageType: 'ANSWER_AND_EXTRACT_FIXATION_WITH_EXPLANATIONS', allowedUrl: GRAN_LESSON_URL, filenameSuffix: 'exercicios-fixacao-com-explicacoes', loadingText: 'Respondendo, abrindo explicações e montando o TXT…' });
});

void loadSavedConnection();
