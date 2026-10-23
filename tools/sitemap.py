"""Writes docs/sitemap.xml, one entry per page, dated by its last git commit.

    python tools/sitemap.py

tools/legal-pages.py runs this too. A page with uncommitted changes is dated
today. Add new pages to PAGES.
"""
import datetime
import subprocess
import sys
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
WEBSITE = "https://oneclickoptimizer.vercel.app"
# File in docs/ -> the clean URL Vercel serves it at.
PAGES = {
    "index.html": "/",
    "terms.html": "/terms",
    "privacy.html": "/privacy",
    "refund.html": "/refund",
    "licenses.html": "/licenses",
}


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True).stdout.strip()


def lastmod(path):
    rel = path.relative_to(ROOT).as_posix()
    if not git("status", "--porcelain", "--", rel):
        committed = git("log", "-1", "--format=%cs", "--", rel)
        if committed:
            return committed
    return datetime.date.today().isoformat()


def main():
    lines = ['<?xml version="1.0" encoding="UTF-8"?>', '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">']
    for name, path in PAGES.items():
        f = DOCS / name
        if not f.exists():
            sys.exit(f"docs/{name} is listed in tools/sitemap.py but doesn't exist")
        lines.append(f"  <url><loc>{escape(WEBSITE + path)}</loc><lastmod>{lastmod(f)}</lastmod></url>")
    lines.append("</urlset>")
    (DOCS / "sitemap.xml").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print("wrote docs/sitemap.xml")


if __name__ == "__main__":
    main()
