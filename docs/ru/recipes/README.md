# Рецепты

Рецепты — это полные, аннотированные примеры Coastfile для распространённых типов проектов. Каждый рецепт включает полный Coastfile, который вы можете скопировать и адаптировать, а затем — пошаговый разбор по разделам, объясняющий, почему было принято каждое решение.

Если вы новичок в Coastfile, сначала ознакомьтесь со справочником [Coastfiles reference](../coastfiles/README.md). Рецепты предполагают знакомство с базовыми концепциями.

- [Full-Stack Monorepo](FULLSTACK_MONOREPO.md) — общий Postgres и Redis на хосте, фронтенды Vite как bare-service и бэкенд в Docker через compose. Рассматриваются стратегии томов, healthchecks, настройка assign и `exclude_paths` для больших репозиториев.
- [Next.js Application](NEXTJS.md) — Next.js с Turbopack, общими Postgres и Redis, фоновыми воркерами и динамической обработкой портов для auth callbacks. Рассматриваются `private_paths` для изоляции `.next`, оптимизация bare service и поддержка worktree для нескольких агентов.
