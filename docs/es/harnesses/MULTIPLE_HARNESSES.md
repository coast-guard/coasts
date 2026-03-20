# Múltiples Harnesses

Si un repositorio se usa desde más de un harness, una forma de consolidar
la configuración de Coasts es mantener el flujo de trabajo compartido de `/coasts` en un solo lugar y mantener
las reglas siempre activas específicas del harness en los archivos de cada harness.

## Disposición recomendada

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

Usa esta disposición así:

- `AGENTS.md` — reglas cortas, siempre activas para trabajar con Coasts en Codex y T3
  Code
- `.cursor/rules/coast.md` — reglas opcionales, siempre activas y nativas de Cursor
- `CLAUDE.md` — reglas cortas, siempre activas para trabajar con Coasts en Claude Code
  y Conductor
- `.agents/skills/coasts/SKILL.md` — flujo de trabajo canónico reutilizable de `/coasts`
- `.agents/skills/coasts/agents/openai.yaml` — metadatos opcionales de Codex/OpenAI
- `.claude/skills/coasts` — espejo o symlink orientado a Claude cuando Claude Code
  también necesita la misma skill
- `.cursor/commands/coasts.md` — archivo de comando opcional de Cursor; una opción
  simple es hacer que reutilice la misma skill
- `.claude/commands/coasts.md` — archivo de comando explícito opcional; una opción
  simple es hacer que reutilice la misma skill

## Paso a paso

1. Coloca las reglas de Coast Runtime en los archivos de instrucciones siempre activos.
   - `AGENTS.md`, `CLAUDE.md` o `.cursor/rules/coast.md` deben responder a las
     reglas de "cada tarea": ejecutar `coast lookup` primero, usar `coast exec`, leer registros
     con `coast logs`, preguntar antes de `coast assign` o `coast run` cuando no haya
     coincidencia.
2. Crea una skill canónica para Coasts.
   - Coloca el flujo de trabajo reutilizable de `/coasts` en `.agents/skills/coasts/SKILL.md`.
   - Usa el Coast CLI directamente dentro de esa skill: `coast lookup`,
     `coast ls`, `coast run`, `coast assign`, `coast unassign`,
     `coast checkout` y `coast ui`.
3. Expón esa skill solo donde un harness necesite una ruta diferente.
   - Codex, T3 Code y Cursor pueden usar `.agents/skills/` directamente.
   - Claude Code necesita `.claude/skills/`, así que refleja o crea un symlink de la
     skill canónica en esa ubicación.
4. Agrega un archivo de comando solo si quieres un punto de entrada `/coasts` explícito.
   - Si creas `.claude/commands/coasts.md` o
     `.cursor/commands/coasts.md`, una opción simple es hacer que el comando
     reutilice la misma skill.
   - Si le das al comando sus propias instrucciones separadas, estás asumiendo una
     segunda copia del flujo de trabajo que mantener.
5. Mantén la configuración específica de Conductor en Conductor, no en la skill.
   - Usa scripts de Repository Settings de Conductor para el comportamiento de bootstrap o ejecución
     que pertenece al propio Conductor.
   - Mantén la política de Coasts y el uso del CLI `coast` en `CLAUDE.md` y la
     skill compartida.

## Ejemplo concreto de `/coasts`

Una buena skill compartida `coasts` debe hacer tres trabajos:

1. `Use Existing Coast`
   - ejecutar `coast lookup`
   - si existe una coincidencia, usar `coast exec`, `coast ps` y `coast logs`
2. `Manage Assignment`
   - ejecutar `coast ls`
   - ofrecer `coast run`, `coast assign`, `coast unassign` o
     `coast checkout`
   - preguntar antes de reutilizar o interrumpir una ranura existente
3. `Open UI`
   - ejecutar `coast ui`

Ese es el lugar correcto para el flujo de trabajo de `/coasts`. Los archivos siempre activos deben
contener solo las reglas cortas que deben aplicarse incluso cuando la skill nunca se invoca.

## Patrón de symlink

Si quieres que Claude Code reutilice la misma skill que Codex, T3 Code o Cursor,
una opción es un symlink:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

Un espejo versionado también está bien si tu equipo prefiere no usar symlinks. El
objetivo principal es simplemente evitar una divergencia innecesaria entre copias.

## Precauciones específicas del harness

- Claude Code: tanto las skills del proyecto como los comandos opcionales del proyecto son válidos, pero
  mantén la lógica en la skill.
- Cursor: usa `AGENTS.md` o `.cursor/rules/coast.md` para las reglas cortas de Coast
  Runtime, usa una skill para el flujo de trabajo reutilizable y mantén
  `.cursor/commands` como opcional.
- Conductor: trátalo primero como `CLAUDE.md` más scripts y configuraciones de Conductor.
  Si agregas un comando y no aparece, cierra completamente y vuelve a abrir la app
  antes de comprobar de nuevo.
- T3 Code: esta es la superficie de harness más delgada aquí. Usa el patrón de estilo Codex
  `AGENTS.md` más `.agents/skills`, y no inventes una disposición de comandos
  separada y específica de T3 para documentación sobre Coasts.
- Codex: mantén `AGENTS.md` corto y coloca el flujo de trabajo reutilizable en
  `.agents/skills`.
