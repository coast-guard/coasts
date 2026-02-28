# Contributing to Coasts

We welcome contributions and appreciate you taking the time to improve Coasts. That said, opening a PR does not guarantee it will be merged. We're a small team with a clear direction for the project, and we review everything carefully.

## No drive-by PRs

PRs that show up out of nowhere with no prior context will be closed automatically. If you want to contribute, start by opening an issue or discussion so we can align on whether the change makes sense before you write any code.

## What we expect in a PR

Every pull request must include:

1. **A Loom video or Equivalent** walking through the change. Show us the bug you're fixing or the feature you're adding, then walk through the code and the reason for the change. This doesn't need to be polished — just talk us through what you did and why. If English is not your native language and you'd prefer to explain the change in your mother tongue, that's completely fine. We will translate the video if the change is a good addition.

2. **A written explanation** in the PR description covering what changed, why, and any trade-offs you considered.

## AI tools are fine

We use AI tools ourselves and it would be hypocritical to reject contributions from people who do the same. Use whatever tools help you write good code. The one requirement is that **you understand what you're submitting**. If you can't explain a piece of your PR in the video walkthrough, we probably won't merge it.

## Consider opening an issue instead

Code is cheap to produce now. If the change you want to make is something you could describe in a clear prompt or a few sentences, you're better off writing that as an issue than opening a PR. We can often implement well-described suggestions faster than we can review, test, and merge external code. Save yourself the effort — write the issue, describe what you want and why, and let us take it from there.

PRs are most valuable when they come with context we don't have: a reproduction of a bug, a niche platform fix, or a domain-specific integration we wouldn't have built ourselves.

## Before you submit

- Check existing issues and PRs to make sure the work isn't already in progress.
- For anything non-trivial, open an issue first to discuss the approach.
- Read the [README](README.md) for build and test instructions.

## Tests and linting

Before opening a PR, run the following from the repo root and make sure everything passes with zero warnings:

```bash
make fix     # auto-format and auto-fix clippy warnings
make lint    # verify formatting and clippy (must be clean)
make test    # run all unit tests (must pass with zero warnings)
```

If `make test` produces any compilation warnings, unused variable warnings, or unused import warnings, fix them before submitting. We treat warnings as defects.

## License

By contributing to Coasts, you agree that your contributions will be licensed under the [MIT License](LICENSE).
