# Arneses

La mayoría de los arneses crean worktrees de git para ejecutar tareas en paralelo. Estos worktrees pueden vivir dentro de tu proyecto o completamente fuera de él. El arreglo [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) de Coast le indica dónde buscar -- incluyendo rutas externas como `~/.codex/worktrees` que requieren montajes bind adicionales.

Cada página a continuación cubre la configuración del Coastfile y cualquier advertencia específica de ese arnés.

| Harness | Worktree location | Page |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
