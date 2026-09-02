# StudyFlash · Importador Gran

## Transferir uma aula inteira

1. Abra o StudyFlash no Chrome e entre na sua conta.
2. Abra uma aula do Gran na mesma janela.
3. Abra a extensão e escolha **Conectar ao StudyFlash**.
4. Em **Salvar dentro de**, escolha a matéria, tópico ou página onde a aula deve ficar.
5. Clique em **Transferir revisão completa**.

A extensão coleta, quando o artefato estiver disponível naquela aula: transcrição,
resumo, revisão de bolso, flashcards, exercícios de fixação (com gabaritos e
explicações) e links dos mapas mentais. O StudyFlash cria uma nova página dentro
do destino escolhido e adiciona os flashcards à Revisão global, vinculados à
página para o botão **Rever conteúdo**. Os exercícios de fixação também viram
cartões de múltipla escolha, com as alternativas, gabarito e explicações.

Para obter o gabarito e as explicações dos exercícios, o Gran exige uma resposta.
Por isso a extensão marca a alternativa A em cada exercício após pedir confirmação;
isso fica registrado no histórico do Gran.

O token usado para a conexão é somente o token de acesso da sessão já aberta do
StudyFlash. Ele fica no armazenamento de sessão do Chrome e é apagado quando o
navegador encerra; a extensão não lê nem armazena a senha ou o token de renovação.

## Desenvolvimento

Além da transferência completa, os botões de exportação individual continuam
gerando arquivos TXT locais para conferência.

## Instalação no Chrome

1. Abra `chrome://extensions`.
2. Ative o **Modo do desenvolvedor**.
3. Clique em **Carregar sem compactação**.
4. Selecione esta pasta: `extensions/studyflash-gran-transcript`.
5. Abra uma videoaula do Gran e clique no ícone da extensão.

## Permissões

- Acesso às videoaulas e às questões do Gran, somente para ler os artefatos da
  aula que está aberta.
- Acesso ao domínio do StudyFlash, para consultar sua árvore de páginas e enviar
  o pacote após sua confirmação.
- Permissão de download para os TXT solicitados.
- Permissão de armazenamento de sessão para manter o token de acesso temporário.
- Não acessa arquivos locais nem envia conteúdo sem o clique em **Transferir
  revisão completa**.
