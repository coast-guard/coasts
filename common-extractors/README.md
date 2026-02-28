# Common Extractors

Reusable secret extractors for Coast. These implement the [custom extractor protocol](../SPEC.md) — each is an executable named `coast-extractor-{name}` that reads JSON params from stdin and outputs the secret value to stdout.

## Installation

Add this directory to your PATH:

```bash
export PATH="/path/to/coast/common-extractors:$PATH"
```

Or symlink individual extractors:

```bash
ln -s /path/to/coast/common-extractors/coast-extractor-keychain /usr/local/bin/
```

## Available Extractors

### `coast-extractor-keychain`

Reads a generic password from the macOS Keychain.

**Parameters:**
- `service` (required): The Keychain service name
- `account` (optional): The Keychain account name (defaults to `$USER`)

**Coastfile example:**
```toml
[secrets.my_api_key]
extractor = "keychain"
service = "My Service"
account = "jamie"
inject = "env:MY_API_KEY"
```

**Common service names:**
- `"Claude Code"` — Anthropic API key used by Claude Code CLI
- `"GitHub CLI"` — GitHub personal access token
