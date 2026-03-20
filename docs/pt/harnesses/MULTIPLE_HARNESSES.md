# Múltiplos Harnesses

Se um repositório é usado por mais de um harness, uma forma de consolidar
a configuração do Coasts é manter o workflow compartilhado de `/coasts` em um lugar e manter
as regras always-on específicas do harness nos arquivos de cada harness.

## Layout recomendado

```text
AGENTS.md
CLAUDE.md
.cursor/rules/coast.md           # optional Cursor-native always-on rules
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md       # optional, thin, harness-specific
.claude/commands/coasts.md   # optional, thin, harness-specific
```

Use este layout assim:

- `AGENTS.md` — regras curtas, always-on, para trabalhar com Coasts no Codex e no T3
  Code
- `.cursor/rules/coast.md` — regras opcionais, always-on nativas do Cursor
- `CLAUDE.md` — regras curtas, always-on, para trabalhar com Coasts no Claude Code
  e no Conductor
- `.agents/skills/coasts/SKILL.md` — workflow `/coasts` reutilizável canônico
- `.agents/skills/coasts/agents/openai.yaml` — metadados opcionais do Codex/OpenAI
- `.claude/skills/coasts` — espelho ou symlink voltado para o Claude quando o Claude Code
  também precisar da mesma skill
- `.cursor/commands/coasts.md` — arquivo de comando opcional do Cursor; uma opção
  simples é fazer com que ele reutilize a mesma skill
- `.claude/commands/coasts.md` — arquivo de comando explícito opcional; uma opção
  simples é fazer com que ele reutilize a mesma skill

## Passo a passo

1. Coloque as regras do Coast Runtime nos arquivos de instruções always-on.
   - `AGENTS.md`, `CLAUDE.md` ou `.cursor/rules/coast.md` devem responder às
     regras de "every task": executar `coast lookup` primeiro, usar `coast exec`, ler logs
     com `coast logs`, pedir antes de `coast assign` ou `coast run` quando não houver
     correspondência.
2. Crie uma skill canônica para Coasts.
   - Coloque o workflow `/coasts` reutilizável em `.agents/skills/coasts/SKILL.md`.
   - Use a CLI do Coast diretamente dentro dessa skill: `coast lookup`,
     `coast ls`, `coast run`, `coast assign`, `coast unassign`,
     `coast checkout` e `coast ui`.
3. Exponha essa skill apenas onde um harness precisar de um caminho diferente.
   - Codex, T3 Code e Cursor podem usar `.agents/skills/` diretamente.
   - Claude Code precisa de `.claude/skills/`, então espelhe ou crie um symlink da
     skill canônica para esse local.
4. Adicione um arquivo de comando apenas se você quiser um ponto de entrada `/coasts` explícito.
   - Se você criar `.claude/commands/coasts.md` ou
     `.cursor/commands/coasts.md`, uma opção simples é fazer com que o comando
     reutilize a mesma skill.
   - Se você der ao comando suas próprias instruções separadas, estará assumindo uma
     segunda cópia do workflow para manter.
5. Mantenha a configuração específica do Conductor no Conductor, não na skill.
   - Use scripts de Repository Settings do Conductor para comportamento de bootstrap ou execução
     que pertença ao próprio Conductor.
   - Mantenha a política de Coasts e o uso da CLI `coast` em `CLAUDE.md` e na
     skill compartilhada.

## Exemplo concreto de `/coasts`

Uma boa skill compartilhada de `coasts` deve fazer três trabalhos:

1. `Use Existing Coast`
   - execute `coast lookup`
   - se existir uma correspondência, use `coast exec`, `coast ps` e `coast logs`
2. `Manage Assignment`
   - execute `coast ls`
   - ofereça `coast run`, `coast assign`, `coast unassign` ou
     `coast checkout`
   - pergunte antes de reutilizar ou interromper um slot existente
3. `Open UI`
   - execute `coast ui`

Esse é o lugar certo para o workflow `/coasts`. Os arquivos always-on devem
conter apenas as regras curtas que precisam se aplicar mesmo quando a skill nunca é invocada.

## Padrão de symlink

Se você quiser que o Claude Code reutilize a mesma skill que Codex, T3 Code ou Cursor,
uma opção é um symlink:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

Um espelho versionado também é válido se sua equipe preferir não usar symlinks. O
objetivo principal é apenas evitar divergência desnecessária entre cópias.

## Cuidados específicos por harness

- Claude Code: skills de projeto e comandos de projeto opcionais são ambos válidos, mas
  mantenha a lógica na skill.
- Cursor: use `AGENTS.md` ou `.cursor/rules/coast.md` para as regras curtas do Coast
  Runtime, use uma skill para o workflow reutilizável e mantenha
  `.cursor/commands` opcional.
- Conductor: trate-o primeiro como `CLAUDE.md` mais scripts e configurações do Conductor.
  Se você adicionar um comando e ele não aparecer, feche completamente e reabra o aplicativo
  antes de verificar novamente.
- T3 Code: esta é a superfície de harness mais enxuta aqui. Use o padrão
  `AGENTS.md` no estilo Codex mais `.agents/skills`, e não invente um layout de comando
  separado e específico do T3 para documentação sobre Coasts.
- Codex: mantenha `AGENTS.md` curto e coloque o workflow reutilizável em
  `.agents/skills`.
