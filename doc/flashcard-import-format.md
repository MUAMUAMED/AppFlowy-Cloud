# Formato de importação de flashcards com IA

O importador aceita um documento JSON no formato `studyflashcards.v1`, com 1 a 200 cartões. O JSON também pode estar dentro de um bloco Markdown `json`.

## Instrução para configurar a IA

Use o texto abaixo como instrução permanente do modelo que processará as imagens e transcrições:

```text
Você transforma imagens, transcrições e materiais de aula em flashcards para o StudyFlash.

Responda SOMENTE com JSON válido. Não escreva introdução, comentários ou texto depois do JSON.
Use exatamente o formato "studyflashcards.v1".

Regras:
1. Crie cartões autossuficientes, claros e sem depender da imagem original.
2. Não invente informações ausentes no material.
3. Elimine cartões repetidos ou que testem exatamente o mesmo conhecimento.
4. Tipos permitidos: "classic", "true_false" e "multiple_choice".
5. Dificuldades permitidas: "muito_facil", "facil", "medio", "dificil", "muito_dificil".
6. classic exige front e back.
7. true_false exige correct_answer booleano (true ou false) e explanation.
8. multiple_choice exige pelo menos 2 choices; correct_answer deve ser idêntica a uma das choices.
9. Use tags curtas, em minúsculas e sem #.
10. Gere no máximo 200 cartões.
```

## Exemplo completo

```json
{
  "format": "studyflashcards.v1",
  "cards": [
    {
      "type": "classic",
      "front": "O que é o crédito rotativo do cartão?",
      "back": "É o financiamento do saldo da fatura que não foi pago integralmente.",
      "explanation": "Use uma resposta curta e autossuficiente.",
      "difficulty": "medio",
      "tags": ["cartao-de-credito"]
    },
    {
      "type": "true_false",
      "front": "O pagamento mínimo quita integralmente a fatura do cartão.",
      "correct_answer": false,
      "explanation": "O saldo restante entra no crédito rotativo ou em parcelamento.",
      "difficulty": "facil",
      "tags": ["cartao-de-credito"]
    },
    {
      "type": "multiple_choice",
      "front": "Qual operação ocorre quando apenas parte da fatura é paga?",
      "choices": ["Débito automático", "Crédito rotativo", "Estorno", "Saque à vista"],
      "correct_answer": "Crédito rotativo",
      "explanation": "O valor não pago é financiado pela instituição emissora.",
      "difficulty": "dificil",
      "tags": ["cartao-de-credito"]
    }
  ]
}
```

Todos os cartões importados entram nos flashcards globais. Quando a importação é aberta dentro de uma página, o usuário pode adicionar também blocos visuais à página e manter o vínculo usado pelo botão **Rever conteúdo**.
