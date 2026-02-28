#!/usr/bin/env python3
"""
Coast documentation localization pipeline.

Tracks English doc changes via per-file SHA-256 hashes, detects stale/missing
translations per locale, and automates translation via the OpenAI API.

Subcommands:
    status    -- Show which docs are missing or stale per locale
    translate -- Translate missing/stale docs for a locale via OpenAI
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional

LOCALES = {
    "zh": "Chinese (Simplified)",
    "ja": "Japanese",
    "ko": "Korean",
    "ru": "Russian",
    "pt": "Portuguese",
    "es": "Spanish",
}

PROJECT_ROOT = Path(__file__).resolve().parent.parent
DOCS_DIR = PROJECT_ROOT / "docs"
TRANSLATION_STATE_DIR = PROJECT_ROOT / "translation-state"
STATE_FILE = TRANSLATION_STATE_DIR / "state.json"

IGNORED_FILES = {".DS_Store"}

DEFAULT_TRANSLATION_MODEL = "gpt-5.2"
OPENAI_MODEL_ENV_VAR = "OPENAI_TRANSLATION_MODEL"
OPENAI_API_KEY_ENV_VAR = "OPENAI_API_KEY"
OPENAI_CHAT_COMPLETIONS_URL = "https://api.openai.com/v1/chat/completions"
OPENAI_TIMEOUT_SECONDS = 300

_PROJECT_ENV_LOADED = False


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def git_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=True,
    )
    return Path(result.stdout.strip())


def git_head_sha() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        capture_output=True, text=True, check=True,
        cwd=DOCS_DIR,
    )
    return result.stdout.strip()


def git_diff(from_commit: str, filepath: Path) -> str:
    """Return the git diff of a file from a recorded commit to HEAD."""
    try:
        result = subprocess.run(
            ["git", "diff", from_commit, "HEAD", "--", str(filepath)],
            capture_output=True, text=True, check=True,
            cwd=git_root(),
        )
        return result.stdout
    except subprocess.CalledProcessError:
        return ""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def discover_english_docs() -> List[Path]:
    """Walk docs/ and return relative paths of English (non-locale) doc files."""
    locale_dirs = set(LOCALES.keys())
    docs = []
    for root, dirs, files in os.walk(DOCS_DIR):
        rel_root = Path(root).relative_to(DOCS_DIR)
        # Skip locale directories at the top level
        if rel_root == Path("."):
            dirs[:] = [d for d in dirs if d not in locale_dirs]
        # Skip hidden dirs
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        for f in sorted(files):
            if f in IGNORED_FILES or f.startswith("."):
                continue
            rel_path = rel_root / f
            docs.append(rel_path)
    docs.sort()
    return docs


def load_state() -> dict:
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text())
    return {}


def save_state(state: dict) -> None:
    TRANSLATION_STATE_DIR.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n")


def load_project_env_if_needed() -> None:
    """Load PROJECT_ROOT/.env into process env only once, without overriding."""
    global _PROJECT_ENV_LOADED
    if _PROJECT_ENV_LOADED:
        return

    env_file = PROJECT_ROOT / ".env"
    if not env_file.exists():
        _PROJECT_ENV_LOADED = True
        return

    try:
        for raw_line in env_file.read_text().splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" not in line:
                continue
            key, value = line.split("=", 1)
            key = key.strip()
            value = value.strip()
            if not key or key in os.environ:
                continue
            if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
                value = value[1:-1]
            os.environ[key] = value
    except OSError as exc:
        print(f"    WARNING: failed to read {env_file}: {exc}")
    finally:
        _PROJECT_ENV_LOADED = True


# ---------------------------------------------------------------------------
# Subcommand: status
# ---------------------------------------------------------------------------

def file_needs_translation(
    state: dict, locale: str, rel_path: str, current_hash: str,
) -> Optional[str]:
    """Return reason string if file needs translation, else None."""
    locale_file = DOCS_DIR / locale / rel_path
    locale_state = state.get(locale, {})
    entry = locale_state.get(rel_path)

    if not locale_file.exists():
        return "missing"
    if locale_file.stat().st_size == 0:
        return "empty"
    if entry is None:
        return "no tracking entry"
    if entry.get("source_sha256") != current_hash:
        return "source changed"
    return None


def cmd_status(args: argparse.Namespace) -> None:
    docs = discover_english_docs()
    state = load_state()
    check_locales = [args.locale] if args.locale else list(LOCALES.keys())

    any_needed = False
    for locale in sorted(check_locales):
        if locale not in LOCALES:
            print(f"Unknown locale: {locale}")
            sys.exit(1)
        lang = LOCALES[locale]
        print(f"\n[{locale}] {lang}")
        print("-" * 40)
        needed = []
        up_to_date = []
        for doc in docs:
            current_hash = sha256_file(DOCS_DIR / doc)
            reason = file_needs_translation(state, locale, str(doc), current_hash)
            if reason:
                needed.append((str(doc), reason))
            else:
                up_to_date.append(str(doc))

        if needed:
            any_needed = True
            for path, reason in needed:
                print(f"  NEEDS TRANSLATION  {path}  ({reason})")
        if up_to_date:
            for path in up_to_date:
                print(f"  up to date         {path}")
        if not needed and not up_to_date:
            print("  (no English docs found)")

    if not any_needed:
        print("\nAll translations are up to date.")


# ---------------------------------------------------------------------------
# Subcommand: translate
# ---------------------------------------------------------------------------

TRANSLATION_START_MARKER = "<!-- TRANSLATION START -->"
TRANSLATION_END_MARKER = "<!-- TRANSLATION END -->"

MAX_RETRIES = 3

STRUCTURE_RULES = """\
- Preserve ALL markdown formatting, headings, code blocks, links, and front matter exactly.
- Do NOT translate text inside code blocks or inline code.
- Do NOT skip, reorder, merge, or drop ANY section. Every heading in the source MUST appear in the translation.
- Do NOT add any preamble, explanation, reasoning, or commentary.
- You MUST wrap your translated document between these exact markers:

{start_marker}
(translated document here)
{end_marker}

Everything outside the markers is ignored. Do NOT include anything else."""

NEW_TRANSLATION_PROMPT = """\
Translate the following documentation from English to {language}.

Rules:
{structure_rules}

--- SOURCE ---
{source}
"""

UPDATE_TRANSLATION_PROMPT = """\
Update the following {language} translation based on changes to the English source.

Rules:
{structure_rules}

--- ENGLISH SOURCE (current) ---
{source}

--- CHANGES SINCE LAST TRANSLATION ---
{diff}

--- CURRENT TRANSLATION ---
{translation}
"""

RETRY_ADDENDUM = """

IMPORTANT: Your previous attempt was rejected: {rejection_reason}
The source document has these headings (you MUST translate ALL of them):
{heading_list}
"""


def _normalize_md(text: str) -> str:
    """Normalize fullwidth markdown characters to ASCII equivalents."""
    return text.replace("\uff03", "#").replace("\uff0a", "*").replace("\uff1a", ":")


def extract_translation(raw_output: str) -> Optional[str]:
    """Extract content between markers. Returns None if markers not found."""
    start_idx = raw_output.find(TRANSLATION_START_MARKER)
    end_idx = raw_output.find(TRANSLATION_END_MARKER)
    if start_idx == -1 or end_idx == -1 or end_idx <= start_idx:
        return None
    content = raw_output[start_idx + len(TRANSLATION_START_MARKER):end_idx]
    # Normalize fullwidth chars that break markdown rendering
    content = _normalize_md(content)
    return content.strip() + "\n"


def _count_headings(text: str) -> Dict[str, int]:
    """Count markdown headings by level (e.g. '#', '##', '###')."""
    counts: Dict[str, int] = {}
    for line in _normalize_md(text).splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            prefix = stripped.split()[0] if stripped.split() else ""
            if prefix and all(c == "#" for c in prefix):
                counts[prefix] = counts.get(prefix, 0) + 1
    return counts


def _list_headings(text: str) -> List[str]:
    """Return all markdown headings as a list of strings."""
    headings = []
    for line in _normalize_md(text).splitlines():
        stripped = line.strip()
        if stripped.startswith("#"):
            parts = stripped.split(None, 1)
            if parts and all(c == "#" for c in parts[0]):
                headings.append(stripped)
    return headings


def validate_translation(source: str, translated: str) -> Optional[str]:
    """Basic sanity checks. Returns error message or None if OK."""
    if not translated.strip():
        return "translated output is empty"

    source_lines = [l for l in source.strip().splitlines() if l.strip()]
    trans_lines = [l for l in translated.strip().splitlines() if l.strip()]

    # If the source starts with a markdown heading, the translation should too
    if source_lines and source_lines[0].startswith("#"):
        if not trans_lines or not trans_lines[0].startswith("#"):
            return "translation does not start with a markdown heading like the source"

    # Heading counts must match -- catches dropped/added sections
    src_headings = _count_headings(source)
    trans_headings = _count_headings(translated)
    for level, count in src_headings.items():
        trans_count = trans_headings.get(level, 0)
        if trans_count != count:
            return (
                f"heading count mismatch for '{level}': "
                f"source has {count}, translation has {trans_count}"
            )

    # Code fence count must match -- catches dropped/mangled blocks
    src_fences = source.count("```")
    trans_fences = translated.count("```")
    if src_fences != trans_fences:
        return (
            f"code fence count mismatch: "
            f"source has {src_fences}, translation has {trans_fences}"
        )

    # Flag obvious hallucination patterns
    hallucination_signals = [
        "here is the",
        "here's the",
        "the translation is",
        "looking at the",
        "i'll translate",
        "let me translate",
        "below is the",
    ]
    first_lines = "\n".join(trans_lines[:5]).lower()
    for signal in hallucination_signals:
        if signal in first_lines:
            return f"translation appears to contain reasoning/commentary (found: '{signal}')"

    return None


def _build_prompt(
    language: str,
    source: str,
    existing_translation: str,
    diff_text: str,
    retry_reason: Optional[str] = None,
) -> str:
    """Construct the translation prompt, optionally with retry hints."""
    rules = STRUCTURE_RULES.format(
        start_marker=TRANSLATION_START_MARKER,
        end_marker=TRANSLATION_END_MARKER,
    )

    if existing_translation and diff_text:
        prompt = UPDATE_TRANSLATION_PROMPT.format(
            language=language,
            source=source,
            diff=diff_text,
            translation=existing_translation,
            structure_rules=rules,
        )
    else:
        prompt = NEW_TRANSLATION_PROMPT.format(
            language=language,
            source=source,
            structure_rules=rules,
        )

    if retry_reason:
        headings = _list_headings(source)
        heading_list = "\n".join(f"  {h}" for h in headings)
        prompt += RETRY_ADDENDUM.format(
            rejection_reason=retry_reason,
            heading_list=heading_list,
        )

    return prompt


def _resolve_translation_model(cli_model: Optional[str]) -> str:
    if cli_model:
        return cli_model
    load_project_env_if_needed()
    return os.environ.get(OPENAI_MODEL_ENV_VAR, DEFAULT_TRANSLATION_MODEL)


def _invoke_openai(prompt: str, model: str) -> Optional[str]:
    """Call OpenAI Chat Completions and return text output or None on error."""
    load_project_env_if_needed()
    api_key = os.environ.get(OPENAI_API_KEY_ENV_VAR)
    if not api_key:
        print(
            f"    ERROR: {OPENAI_API_KEY_ENV_VAR} is not set. "
            "Export it or add it to .env in the project root."
        )
        return None

    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
    }
    req = urllib.request.Request(
        OPENAI_CHAT_COMPLETIONS_URL,
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=OPENAI_TIMEOUT_SECONDS) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        error_body = exc.read().decode("utf-8", errors="replace")
        print(f"    ERROR: OpenAI API error {exc.code}")
        if error_body:
            print(f"    response: {error_body[:500]}")
        return None
    except urllib.error.URLError as exc:
        print(f"    ERROR: OpenAI request failed: {exc.reason}")
        return None
    except TimeoutError:
        print(f"    ERROR: OpenAI request timed out after {OPENAI_TIMEOUT_SECONDS}s")
        return None

    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        print("    ERROR: OpenAI response was not valid JSON")
        print(f"    response: {raw[:500]}")
        return None

    choices = payload.get("choices")
    if not choices:
        print("    ERROR: OpenAI response missing choices")
        print(f"    response: {raw[:500]}")
        return None

    message = choices[0].get("message", {})
    content = message.get("content")
    if isinstance(content, str):
        return content

    if isinstance(content, list):
        text_chunks = []
        for part in content:
            if not isinstance(part, dict):
                continue
            if part.get("type") in {"text", "output_text"} and isinstance(part.get("text"), str):
                text_chunks.append(part["text"])
        if text_chunks:
            return "".join(text_chunks)

    print("    ERROR: OpenAI response missing text content")
    print(f"    response: {raw[:500]}")
    return None


def translate_file(
    locale: str,
    language: str,
    rel_path: str,
    dry_run: bool,
    model: str,
) -> bool:
    """Translate a single file with retries. Returns True on success."""
    english_file = DOCS_DIR / rel_path
    locale_file = DOCS_DIR / locale / rel_path
    source = english_file.read_text()

    if not source.strip():
        print(f"  SKIP {rel_path} (English source is empty)")
        return False

    state = load_state()
    entry = state.get(locale, {}).get(rel_path)

    existing_translation = ""
    if locale_file.exists() and locale_file.stat().st_size > 0:
        existing_translation = locale_file.read_text()

    diff_text = ""
    if entry and entry.get("commit"):
        diff_text = git_diff(entry["commit"], DOCS_DIR / rel_path)

    if dry_run:
        prompt = _build_prompt(language, source, existing_translation, diff_text)
        print(f"  DRY RUN {rel_path}")
        print(f"    model: {model}")
        print(f"    prompt length: {len(prompt)} chars")
        return True

    print(f"  Translating {rel_path} -> {locale}/...")

    retry_reason = None
    for attempt in range(1, MAX_RETRIES + 1):
        prompt = _build_prompt(
            language, source, existing_translation, diff_text, retry_reason,
        )

        raw = _invoke_openai(prompt, model)
        if raw is None:
            return False

        translated = extract_translation(raw)
        if translated is None:
            print(f"    WARNING: output markers not found, using raw output")
            translated = raw.strip() + "\n"

        error = validate_translation(source, translated)
        if error is None:
            locale_file.parent.mkdir(parents=True, exist_ok=True)
            locale_file.write_text(translated)
            if attempt > 1:
                print(f"    Passed on attempt {attempt}")
            print(f"    Wrote {locale_file}")
            return True

        if attempt < MAX_RETRIES:
            print(f"    Attempt {attempt}/{MAX_RETRIES} rejected: {error} -- retrying")
            retry_reason = error
        else:
            print(f"    REJECTED after {MAX_RETRIES} attempts: {error}")
            print(f"    Raw output saved to /tmp/coast-translate-rejected.txt")
            Path("/tmp/coast-translate-rejected.txt").write_text(raw)

    return False


def cmd_translate(args: argparse.Namespace) -> None:
    locale = args.locale
    if locale not in LOCALES:
        print(f"Unknown locale: {locale}. Supported: {', '.join(LOCALES.keys())}")
        sys.exit(1)

    language = LOCALES[locale]
    model = _resolve_translation_model(args.model)
    docs = discover_english_docs()
    state = load_state()

    # Filter to files that need translation
    targets: List[str] = []
    if args.file:
        rel = args.file
        if not (DOCS_DIR / rel).exists():
            print(f"English source not found: {DOCS_DIR / rel}")
            sys.exit(1)
        targets.append(rel)
    else:
        for doc in docs:
            current_hash = sha256_file(DOCS_DIR / doc)
            reason = file_needs_translation(state, locale, str(doc), current_hash)
            if reason:
                targets.append(str(doc))

    if not targets:
        print(f"[{locale}] All translations are up to date.")
        return

    print(f"[{locale}] {language}: {len(targets)} file(s) to translate")
    print(f"Using model: {model}\n")

    commit = git_head_sha()
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    success_count = 0

    for rel_path in targets:
        ok = translate_file(locale, language, rel_path, args.dry_run, model)
        if ok and not args.dry_run:
            current_hash = sha256_file(DOCS_DIR / rel_path)
            if locale not in state:
                state[locale] = {}
            state[locale][rel_path] = {
                "source_sha256": current_hash,
                "commit": commit,
                "translated_at": now,
            }
            save_state(state)
            success_count += 1

    print(f"\nDone. {success_count}/{len(targets)} file(s) translated for [{locale}].")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Coast docs localization pipeline",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    status_p = sub.add_parser("status", help="Show translation status")
    status_p.add_argument("--locale", help="Check a single locale")

    translate_p = sub.add_parser("translate", help="Translate docs for a locale")
    translate_p.add_argument("--locale", required=True, help="Target locale (e.g. es, zh)")
    translate_p.add_argument("--file", help="Translate a single file (relative to docs/)")
    translate_p.add_argument("--dry-run", action="store_true", help="Show what would be translated")
    translate_p.add_argument(
        "--model",
        help=(
            f"OpenAI model name (default: {DEFAULT_TRANSLATION_MODEL}; "
            f"or set ${OPENAI_MODEL_ENV_VAR})"
        ),
    )

    args = parser.parse_args()

    if args.command == "status":
        cmd_status(args)
    elif args.command == "translate":
        cmd_translate(args)


if __name__ == "__main__":
    main()
