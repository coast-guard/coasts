# Несколько harness-ов

Если один репозиторий используется более чем из одного harness-а, один из
способов централизовать настройку Coasts — хранить общий workflow `/coasts` в
одном месте, а специфичные для harness-а always-on rules — в файлах каждого
harness-а.

## Рекомендуемая структура

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

Используйте эту структуру следующим образом:

- `AGENTS.md` — короткие always-on rules для работы с Coasts в Codex и T3
  Code
- `.cursor/rules/coast.md` — необязательные Cursor-native always-on rules
- `CLAUDE.md` — короткие always-on rules для работы с Coasts в Claude Code
  и Conductor
- `.agents/skills/coasts/SKILL.md` — канонический переиспользуемый workflow `/coasts`
- `.agents/skills/coasts/agents/openai.yaml` — необязательные метаданные Codex/OpenAI
- `.claude/skills/coasts` — зеркало или symlink для Claude, когда Claude Code
  также нужен тот же skill
- `.cursor/commands/coasts.md` — необязательный файл команды Cursor; один из
  простых вариантов — переиспользовать тот же skill
- `.claude/commands/coasts.md` — необязательный явный файл команды; один из
  простых вариантов — переиспользовать тот же skill

## Пошагово

1. Поместите правила Coast Runtime в always-on instruction files.
   - `AGENTS.md`, `CLAUDE.md` или `.cursor/rules/coast.md` должны отвечать за
     правила для "каждой задачи": сначала запускать `coast lookup`,
     использовать `coast exec`, читать логи через `coast logs`, спрашивать
     перед `coast assign` или `coast run`, если совпадения нет.
2. Создайте один канонический skill для Coasts.
   - Поместите переиспользуемый workflow `/coasts` в `.agents/skills/coasts/SKILL.md`.
   - Используйте Coast CLI напрямую внутри этого skill: `coast lookup`,
     `coast ls`, `coast run`, `coast assign`, `coast unassign`,
     `coast checkout` и `coast ui`.
3. Показывайте этот skill только там, где harness-у нужен другой путь.
   - Codex, T3 Code и Cursor могут использовать `.agents/skills/` напрямую.
   - Claude Code нужен `.claude/skills/`, поэтому отзеркальте или создайте
     symlink на канонический skill в этом расположении.
4. Добавляйте файл команды только если вам нужна явная точка входа `/coasts`.
   - Если вы создаёте `.claude/commands/coasts.md` или
     `.cursor/commands/coasts.md`, один из простых вариантов — чтобы команда
     переиспользовала тот же skill.
   - Если вы даёте команде собственные отдельные инструкции, вы берёте на себя
     поддержку второй копии workflow.
5. Храните специфичную для Conductor настройку в Conductor, а не в skill.
   - Используйте скрипты Conductor Repository Settings для bootstrap или
     поведения при запуске, которое относится к самому Conductor.
   - Храните политику Coasts и использование `coast` CLI в `CLAUDE.md` и
     общем skill.

## Конкретный пример `/coasts`

Хороший общий skill `coasts` должен выполнять три задачи:

1. `Use Existing Coast`
   - запустить `coast lookup`
   - если совпадение существует, использовать `coast exec`, `coast ps` и `coast logs`
2. `Manage Assignment`
   - запустить `coast ls`
   - предложить `coast run`, `coast assign`, `coast unassign` или
     `coast checkout`
   - спрашивать перед повторным использованием или нарушением работы
     существующего slot
3. `Open UI`
   - запустить `coast ui`

Это правильное место для workflow `/coasts`. Файлы always-on должны
содержать только короткие правила, которые должны применяться даже если skill
никогда не вызывается.

## Шаблон symlink

Если вы хотите, чтобы Claude Code переиспользовал тот же skill, что и Codex,
T3 Code или Cursor, один из вариантов — symlink:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

Закоммиченное зеркало тоже подходит, если ваша команда предпочитает не
использовать symlink. Главная цель — просто избежать ненужного расхождения
между копиями.

## Предостережения для конкретных harness-ов

- Claude Code: project skills и необязательные project commands оба допустимы, но
  держите логику в skill.
- Cursor: используйте `AGENTS.md` или `.cursor/rules/coast.md` для коротких
  правил Coast Runtime, используйте skill для переиспользуемого workflow и
  оставляйте `.cursor/commands` необязательными.
- Conductor: в первую очередь рассматривайте его как `CLAUDE.md` плюс скрипты
  и настройки Conductor.
  Если вы добавили команду и она не появляется, полностью закройте и снова
  откройте приложение, прежде чем проверять ещё раз.
- T3 Code: это самый тонкий surface harness-а здесь. Используйте шаблон в
  стиле Codex: `AGENTS.md` плюс `.agents/skills`, и не придумывайте отдельную
  T3-специфичную структуру команд для документации о Coasts.
- Codex: держите `AGENTS.md` коротким и поместите переиспользуемый workflow в
  `.agents/skills`.
