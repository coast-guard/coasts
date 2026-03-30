# Recetas

Las recetas son ejemplos completos y anotados de Coastfile para formas comunes de proyectos. Cada receta incluye un Coastfile completo que puedes copiar y adaptar, seguido de una guía sección por sección que explica por qué se tomó cada decisión.

Si eres nuevo en los Coastfiles, comienza primero con la [referencia de Coastfiles](../coastfiles/README.md). Las recetas asumen familiaridad con los conceptos principales.

- [Monorepo Full-Stack](FULLSTACK_MONOREPO.md) - Postgres y Redis compartidos en el host, frontends Vite como servicios bare y un backend dockerizado mediante compose. Cubre estrategias de volúmenes, healthchecks, ajuste de assign y `exclude_paths` para repositorios grandes.
- [Aplicación Next.js](NEXTJS.md) - Next.js con Turbopack, Postgres y Redis compartidos, workers en segundo plano y manejo dinámico de puertos para callbacks de autenticación. Cubre `private_paths` para el aislamiento de `.next`, optimización de servicios bare y soporte de worktree multiagente.
