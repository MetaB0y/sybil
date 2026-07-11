#!/usr/bin/env python3
"""Check external links in maintained Markdown without failing on network noise.

Archived implementation plans are intentionally excluded. A confirmed 404 or
410 fails the check; authentication, rate limits, and transient network errors
are reported as warnings because they do not prove that a link is dead.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import ipaddress
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
EXCLUDED_PARTS = {
    ".git",
    ".jj",
    ".next",
    ".venv",
    "node_modules",
    "target",
}
EXCLUDED_PREFIXES = (
    Path("design/archive"),
    Path("frontend/archive"),
)
FENCE_RE = re.compile(r"```.*?```|~~~.*?~~~", re.DOTALL)
INLINE_CODE_RE = re.compile(r"`[^`\n]*`")
URL_RE = re.compile(r"https?://[^\s<>\"'`]+")
TRAILING_PUNCTUATION = ".,;:!?)]}*"
REPO_BLOB_PREFIX = "/MetaB0y/sybil/blob/main/"
REACHABLE_HTTP_ERRORS = {401, 403, 405, 429}
HARD_FAILURES = {404, 410}


def maintained_markdown() -> list[Path]:
    files: list[Path] = []
    for path in ROOT.rglob("*.md"):
        relative = path.relative_to(ROOT)
        if any(part in EXCLUDED_PARTS for part in relative.parts):
            continue
        if any(
            relative == prefix or prefix in relative.parents
            for prefix in EXCLUDED_PREFIXES
        ):
            continue
        files.append(path)
    return sorted(files)


def extract_links(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    text = FENCE_RE.sub("", text)
    text = INLINE_CODE_RE.sub("", text)
    links: set[str] = set()
    for match in URL_RE.finditer(text):
        url = match.group(0).rstrip(TRAILING_PUNCTUATION)
        # Markdown titles are outside the matched URL; strip the common close.
        links.add(url)
    return links


def is_non_public(url: str) -> bool:
    parsed = urllib.parse.urlsplit(url)
    host = parsed.hostname
    if not host:
        return True
    if host in {"localhost", "example.com", "example.org", "example.net"}:
        return True
    if host.endswith(".nip.io") or host.endswith(".test") or host.endswith(".invalid"):
        return True
    try:
        return ipaddress.ip_address(host).is_private
    except ValueError:
        return False


def repository_path(url: str) -> Path | None:
    """Map this repository's GitHub blob URLs to their checkout path."""
    parsed = urllib.parse.urlsplit(url)
    if parsed.hostname != "github.com" or not parsed.path.startswith(REPO_BLOB_PREFIX):
        return None
    relative = urllib.parse.unquote(parsed.path.removeprefix(REPO_BLOB_PREFIX))
    candidate = (ROOT / relative).resolve()
    try:
        candidate.relative_to(ROOT)
    except ValueError:
        return Path("/__invalid_repo_link__")
    return candidate


def request(url: str, method: str) -> int:
    req = urllib.request.Request(
        url,
        method=method,
        headers={
            "User-Agent": "sybil-doc-link-check/1.0 (+https://github.com/MetaB0y/sybil)",
            "Accept": "text/html,application/xhtml+xml,*/*;q=0.8",
        },
    )
    with urllib.request.urlopen(req, timeout=15) as response:
        return response.status


def check(url: str) -> tuple[str, str]:
    # Fragments are local to the target document and often rejected by bots;
    # availability checking only needs the resource URL.
    parsed = urllib.parse.urlsplit(url)
    target = urllib.parse.urlunsplit(parsed._replace(fragment=""))
    try:
        status = request(target, "HEAD")
        return "ok", f"{status}"
    except urllib.error.HTTPError as error:
        if error.code in REACHABLE_HTTP_ERRORS:
            return "ok", f"{error.code} (reachable)"
        # Some origins implement HEAD incorrectly. Confirm any error with GET.
        try:
            status = request(target, "GET")
            return "ok", f"{status}"
        except urllib.error.HTTPError as get_error:
            if get_error.code in REACHABLE_HTTP_ERRORS:
                return "ok", f"{get_error.code} (reachable)"
            if get_error.code in HARD_FAILURES:
                return "hard", str(get_error.code)
            return "warn", f"HTTP {get_error.code}"
        except (urllib.error.URLError, TimeoutError) as get_error:
            return "warn", str(
                get_error.reason
                if isinstance(get_error, urllib.error.URLError)
                else get_error
            )
    except (urllib.error.URLError, TimeoutError) as error:
        return "warn", str(
            error.reason if isinstance(error, urllib.error.URLError) else error
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path, help="Markdown files to check")
    parser.add_argument("--workers", type=int, default=12)
    args = parser.parse_args()

    files = (
        [path.resolve() for path in args.paths] if args.paths else maintained_markdown()
    )
    sources: dict[str, set[str]] = {}
    repository_links = 0
    repository_errors: list[tuple[str, str]] = []
    for path in files:
        for url in extract_links(path):
            local_target = repository_path(url)
            if local_target is not None:
                repository_links += 1
                if not local_target.is_file():
                    repository_errors.append((url, str(path.relative_to(ROOT))))
                continue
            if is_non_public(url):
                continue
            sources.setdefault(url, set()).add(str(path.relative_to(ROOT)))

    print(
        f"Checking {len(sources)} public links and {repository_links} repository links "
        f"from {len(files)} maintained Markdown files"
    )
    results: dict[str, tuple[str, str]] = {}
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, args.workers)
    ) as pool:
        future_urls = {pool.submit(check, url): url for url in sources}
        for future in concurrent.futures.as_completed(future_urls):
            url = future_urls[future]
            try:
                results[url] = future.result()
            except (
                Exception
            ) as error:  # keep one unexpected origin from aborting the audit
                results[url] = ("warn", repr(error))

    hard = len(repository_errors)
    warnings = 0
    for url, location in repository_errors:
        print(f"ERROR missing repository target: {url} ({location})")
    for url in sorted(results):
        outcome, detail = results[url]
        if outcome == "ok":
            continue
        locations = ", ".join(sorted(sources[url]))
        if outcome == "hard":
            hard += 1
            print(f"ERROR {detail}: {url} ({locations})")
        else:
            warnings += 1
            print(f"WARN  {detail}: {url} ({locations})")

    reachable = len(sources) - (hard - len(repository_errors)) - warnings
    print(
        f"Links: {reachable} public reachable, {repository_links - len(repository_errors)} "
        f"repository targets present, {warnings} warnings, {hard} dead"
    )
    return 1 if hard else 0


if __name__ == "__main__":
    sys.exit(main())
