#!/usr/bin/env python3
"""
Generate per-locale search indexes for Coast documentation.

Usage:
    python3 scripts/generate-search-index.py --locale en

Reads coast-guard/src/generated/docs-manifest.json, chunks each doc by heading,
embeds via OpenAI text-embedding-3-large, builds a BM25 inverted index + semantic
neighbor map, and writes:
    search-indexes/docs-search-index-{locale}.json
    embeddings/docs-embeddings-{locale}.json

Requires OPENAI_API_KEY in the environment or in the project root .env file.
"""

import argparse
import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, List

PROJECT_ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = PROJECT_ROOT / "coast-guard" / "src" / "generated" / "docs-manifest.json"
SEARCH_INDEX_DIR = PROJECT_ROOT / "search-indexes"
EMBEDDINGS_DIR = PROJECT_ROOT / "embeddings"

OPENAI_EMBEDDINGS_URL = "https://api.openai.com/v1/embeddings"
EMBEDDING_MODEL = "text-embedding-3-large"
EMBEDDING_BATCH_SIZE = 100
EMBEDDING_BATCH_DELAY = 0.2
EMBEDDING_TIMEOUT = 120

SEMANTIC_TOP_K = 5

_CJK_PATTERN = re.compile(
    r"[\u3000-\u303F\u3040-\u309F\u30A0-\u30FF"
    r"\u4E00-\u9FFF\uAC00-\uD7AF\uFF00-\uFFEF]"
)

_TOKEN_SPLIT = re.compile(r"[^a-zA-Z0-9\u00C0-\u024F\u0400-\u04FF]+")

STOP_WORDS = frozenset([
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of",
    "with", "by", "is", "are", "was", "were", "be", "been", "being", "have",
    "has", "had", "do", "does", "did", "will", "would", "shall", "should",
    "may", "might", "can", "could", "this", "that", "these", "those", "it",
    "its", "not", "no", "nor", "so", "if", "then", "than", "from", "up",
    "out", "as", "into", "about", "each", "which", "their", "there", "your",
    "you", "we", "they", "he", "she", "de", "la", "el", "en", "es", "un",
    "una", "los", "las", "del", "por", "con", "se", "que", "das", "dos",
    "uma", "les", "des", "est", "sur", "du",
])

_PROJECT_ENV_LOADED = False


# ---------------------------------------------------------------------------
# .env loading (same pattern as docs-i18n.py)
# ---------------------------------------------------------------------------

def load_project_env_if_needed() -> None:
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
        print(f"WARNING: failed to read {env_file}: {exc}", file=sys.stderr)
    finally:
        _PROJECT_ENV_LOADED = True


# ---------------------------------------------------------------------------
# Tokenizer — must match the daemon's tokenizer in handlers/docs.rs
# ---------------------------------------------------------------------------

def tokenize(text: str) -> List[str]:
    lower = text.lower()
    tokens: List[str] = []

    for word in _TOKEN_SPLIT.split(lower):
        if word and len(word) >= 2 and word not in STOP_WORDS:
            tokens.append(word)

    for i in range(len(lower) - 1):
        if _CJK_PATTERN.match(lower[i]) and _CJK_PATTERN.match(lower[i + 1]):
            tokens.append(lower[i] + lower[i + 1])

    return tokens


# ---------------------------------------------------------------------------
# Chunking — split a doc into heading-delimited sections
# ---------------------------------------------------------------------------

_HEADING_RE = re.compile(r"^(#{1,3})\s+(.+)")


def file_path_to_route(file_path: str) -> str:
    route = re.sub(r"\.md$", "", file_path)
    if route.endswith("/README") or route == "README":
        return "/docs/" + re.sub(r"/?README$", "", route)
    return "/docs/" + route


def chunk_by_heading(file_path: str, markdown: str) -> List[Dict[str, Any]]:
    lines = markdown.split("\n")
    sections: List[Dict[str, Any]] = []
    current_heading = ""
    current_lines: List[str] = []
    route = file_path_to_route(file_path)

    def flush() -> None:
        content = "\n".join(current_lines).strip()
        if content:
            sections.append({
                "filePath": file_path,
                "heading": current_heading,
                "content": content,
                "route": route,
            })

    for line in lines:
        m = _HEADING_RE.match(line)
        if m:
            flush()
            current_heading = m.group(2).strip()
            current_lines = [line]
        else:
            current_lines.append(line)

    flush()
    return sections


# ---------------------------------------------------------------------------
# OpenAI embeddings
# ---------------------------------------------------------------------------

def get_embeddings(texts: List[str], api_key: str) -> List[List[float]]:
    all_embeddings: List[List[float]] = []

    for i in range(0, len(texts), EMBEDDING_BATCH_SIZE):
        batch = texts[i:i + EMBEDDING_BATCH_SIZE]
        body = json.dumps({
            "model": EMBEDDING_MODEL,
            "input": batch,
        }).encode("utf-8")

        req = urllib.request.Request(
            OPENAI_EMBEDDINGS_URL,
            data=body,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=EMBEDDING_TIMEOUT) as resp:
                raw = resp.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            print(f"ERROR: OpenAI API error {exc.code}: {error_body[:500]}", file=sys.stderr)
            sys.exit(1)
        except urllib.error.URLError as exc:
            print(f"ERROR: OpenAI request failed: {exc.reason}", file=sys.stderr)
            sys.exit(1)

        payload = json.loads(raw)
        sorted_data = sorted(payload["data"], key=lambda x: x["index"])
        for item in sorted_data:
            all_embeddings.append(item["embedding"])

        if i + EMBEDDING_BATCH_SIZE < len(texts):
            time.sleep(EMBEDDING_BATCH_DELAY)

    return all_embeddings


# ---------------------------------------------------------------------------
# Cosine similarity + neighbor computation
# ---------------------------------------------------------------------------

def cosine_similarity(a: List[float], b: List[float]) -> float:
    dot = 0.0
    mag_a = 0.0
    mag_b = 0.0
    for i in range(len(a)):
        dot += a[i] * b[i]
        mag_a += a[i] * a[i]
        mag_b += b[i] * b[i]
    denom = math.sqrt(mag_a) * math.sqrt(mag_b)
    return dot / denom if denom != 0 else 0.0


def compute_semantic_neighbors(
    embeddings: Dict[int, List[float]],
    top_k: int = SEMANTIC_TOP_K,
) -> Dict[str, List[Dict[str, Any]]]:
    ids = sorted(embeddings.keys())
    neighbors: Dict[str, List[Dict[str, Any]]] = {}

    for section_id in ids:
        scores = []
        for other_id in ids:
            if other_id == section_id:
                continue
            score = cosine_similarity(embeddings[section_id], embeddings[other_id])
            scores.append({"s": other_id, "score": round(score * 10000) / 10000})
        scores.sort(key=lambda x: x["score"], reverse=True)
        neighbors[str(section_id)] = scores[:top_k]

    return neighbors


# ---------------------------------------------------------------------------
# BM25 inverted index
# ---------------------------------------------------------------------------

def build_inverted_index(
    sections: List[Dict[str, Any]],
) -> Dict[str, Any]:
    index: Dict[str, List[Dict[str, int]]] = {}

    for section in sections:
        tokens = tokenize(section["heading"] + " " + section["content"])
        section["tokenCount"] = len(tokens)

        tf: Dict[str, int] = {}
        for t in tokens:
            tf[t] = tf.get(t, 0) + 1

        for term, count in tf.items():
            if term not in index:
                index[term] = []
            index[term].append({"s": section["id"], "tf": count})

    doc_count = len(sections)
    idf: Dict[str, float] = {}
    for term, postings in index.items():
        df = len(postings)
        idf[term] = round(math.log((doc_count - df + 0.5) / (df + 0.5) + 1) * 10000) / 10000

    total_tokens = sum(s["tokenCount"] for s in sections)
    avg_dl = round(total_tokens / max(len(sections), 1) * 100) / 100

    return {"invertedIndex": index, "idf": idf, "avgDl": avg_dl}


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate per-locale docs search index",
    )
    parser.add_argument("--locale", required=True, help="Locale code (e.g. en, es, zh)")
    args = parser.parse_args()

    locale: str = args.locale

    load_project_env_if_needed()
    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        print(
            "ERROR: OPENAI_API_KEY is not set. "
            "Export it or add it to .env in the project root.",
            file=sys.stderr,
        )
        sys.exit(1)

    if not MANIFEST_PATH.exists():
        print(
            f"ERROR: docs manifest not found at {MANIFEST_PATH}. "
            "Run 'npm run generate:docs' in coast-guard first.",
            file=sys.stderr,
        )
        sys.exit(1)

    manifest = json.loads(MANIFEST_PATH.read_text())
    locale_data = manifest.get("locales", {}).get(locale)
    if not locale_data:
        print(f'ERROR: Locale "{locale}" not found in docs-manifest.json', file=sys.stderr)
        sys.exit(1)

    print(f"[{locale}] Chunking docs into sections...")
    sections: List[Dict[str, Any]] = []
    for file_path, content in locale_data.get("files", {}).items():
        chunks = chunk_by_heading(file_path, content)
        for chunk in chunks:
            chunk["id"] = len(sections)
            sections.append(chunk)

    if not sections:
        print(f"[{locale}] No sections found -- skipping.")
        return

    print(f"[{locale}] {len(sections)} sections found. Generating embeddings...")
    texts = [f"{s['heading']}\n\n{s['content']}"[:8000] for s in sections]
    embedding_vectors = get_embeddings(texts, api_key)

    embeddings_map: Dict[int, List[float]] = {}
    for i in range(len(sections)):
        embeddings_map[i] = embedding_vectors[i]

    print(f"[{locale}] Computing semantic neighbors...")
    semantic_neighbors = compute_semantic_neighbors(embeddings_map)

    print(f"[{locale}] Building inverted index...")
    index_data = build_inverted_index(sections)

    search_index = {
        "locale": locale,
        "sections": [
            {
                "id": s["id"],
                "filePath": s["filePath"],
                "heading": s["heading"],
                "content": s["content"],
                "route": s["route"],
                "tokenCount": s["tokenCount"],
            }
            for s in sections
        ],
        "invertedIndex": index_data["invertedIndex"],
        "idf": index_data["idf"],
        "semanticNeighbors": semantic_neighbors,
        "avgDl": index_data["avgDl"],
    }

    SEARCH_INDEX_DIR.mkdir(parents=True, exist_ok=True)
    index_path = SEARCH_INDEX_DIR / f"docs-search-index-{locale}.json"
    index_path.write_text(json.dumps(search_index, indent=2) + "\n")
    print(f"[{locale}] Wrote {index_path}")

    EMBEDDINGS_DIR.mkdir(parents=True, exist_ok=True)
    emb_path = EMBEDDINGS_DIR / f"docs-embeddings-{locale}.json"
    emb_map_str = {str(k): v for k, v in embeddings_map.items()}
    emb_path.write_text(json.dumps(emb_map_str) + "\n")
    print(f"[{locale}] Wrote {emb_path}")

    print(f"[{locale}] Done.")


if __name__ == "__main__":
    main()
