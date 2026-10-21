"""Builds the website's legal pages from the texts in legal/.

    python tools/legal-pages.py             writes docs/terms.html, privacy.html,
                                            refund.html, licenses.html and the
                                            footer of docs/index.html
    python tools/legal-pages.py --preview   same, with missing details marked

Edit the wording only in legal/EULA.txt, legal/PRIVACY.md, legal/REFUND.md and
THIRD-PARTY-NOTICES.txt, then run this again. The seller details below fill in
every [BRACKET]; without them the script refuses to publish.
"""
import html
import re
import sys
from pathlib import Path

SELLER = "Ashutosh Roy"  # your legal name, shown as the seller
CITY = "Gurugram, Haryana"  # your city in India, for the license agreement
EMAIL = "mitovoid.contact@gmail.com"  # where customers write to you
WEBSITE = "https://oneclickoptimizer.vercel.app"
UPDATED = "21 October 2026"

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
PREVIEW = "--preview" in sys.argv

# ------------------------------------------------------------------ text to HTML


def inline(s):
    s = html.escape(s, quote=False)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)
    return re.sub(r"(https?://[^\s<]+[^\s<.,;:)])", r'<a href="\1">\1</a>', s)


def para(text):
    cls = ' class="updated"' if text.startswith("Last updated:") else ""
    return f"<p{cls}>{inline(text)}</p>"


def markdown(src):
    """The small part of Markdown that legal/*.md uses."""
    title, out, buf, items, rows = "", [], [], [], []
    tag = ""

    def flush():
        nonlocal buf, items, rows
        if buf:
            out.append(para(" ".join(buf)))
        if items:
            out.append(f"<{tag}>" + "".join(f"<li>{inline(i)}</li>" for i in items) + f"</{tag}>")
        if rows:
            cells = [[c.strip() for c in r.strip().strip("|").split("|")] for r in rows if not re.match(r"^\|[\s|:-]+\|$", r.strip())]
            head = "".join(f'<th scope="col">{inline(c)}</th>' for c in cells[0])
            body = "".join("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in r) + "</tr>" for r in cells[1:])
            out.append(f'<div class="table"><table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div>')
        buf, items, rows = [], [], []

    for line in src.splitlines():
        if line.startswith("# "):
            title = line[2:].strip()
        elif line.startswith("## "):
            flush()
            out.append(f"<h2>{inline(line[3:].strip())}</h2>")
        elif line.startswith("|"):
            if buf or items:
                flush()
            rows.append(line)
        elif m := re.match(r"(\d+\.|-) (.*)", line):
            if buf or rows:
                flush()
            tag = "ul" if m[1] == "-" else "ol"
            items.append(m[2])
        elif not line.strip():
            flush()
        else:
            if rows:
                flush()
            buf.append(line.strip())
    flush()
    return title, "\n".join(out)


KEEP_UPPER = {"PC"}


def sentence(heading):
    words = [w if w in KEEP_UPPER else w.lower() for w in heading.split()]
    words[0] = words[0][0].upper() + words[0][1:]
    return " ".join(words)


def eula(src):
    """legal/EULA.txt: numbered sections in capitals, indented bodies."""
    out, buf, items = [], [], []

    def flush():
        nonlocal buf, items
        if buf:
            out.append(para(" ".join(buf)))
        if items:
            out.append('<ol class="alpha">' + "".join(f"<li>{inline(i)}</li>" for i in items) + "</ol>")
        buf, items = [], []

    for line in src.splitlines()[1:]:
        if m := re.match(r"^(\d+)\. ([A-Z][A-Z ,'-]+)$", line):
            flush()
            out.append(f"<h2>{m[1]}. {html.escape(sentence(m[2]))}</h2>")
        elif m := re.match(r"^\s+([a-z])\. (.*)", line):
            if buf:
                flush()
            items.append(m[2].strip())
        elif not line.strip():
            flush()
        elif items and line.startswith("      "):
            items[-1] += " " + line.strip()
        else:
            if items:
                flush()
            buf.append(line.strip())
    flush()
    return "\n".join(out)


def notices(src):
    """THIRD-PARTY-NOTICES.txt: an intro, then one license per component."""
    parts = re.split(r"\r?\n=+\r?\n(.+?)\r?\n=+\r?\n", src)
    out = [para(" ".join(l.strip() for l in parts[0].splitlines() if l.strip()))]
    for name, text in zip(parts[1::2], parts[2::2]):
        crate, _, version = name.strip().partition(" ")
        out.append(f"<h2>{html.escape(crate)} <span>{html.escape(version)}</span></h2>")
        paras = [" ".join(p.split()) for p in re.split(r"\n\s*\n", text.strip()) if p.strip()]
        out.append('<div class="license">' + "".join(f"<p>{html.escape(p)}</p>" for p in paras) + "</div>")
    return "\n".join(out)


# ------------------------------------------------------------------ details


def fill(s):
    def value(v, label, shown=None):
        if v:
            return shown or html.escape(v)
        if PREVIEW:
            return f"<mark>{label}</mark>"
        sys.exit(f"Fill in {label} at the top of tools/legal-pages.py first.")

    host = WEBSITE.split("://", 1)[1]
    s = re.sub(r"\[WEBSITE\]/(\w+)", lambda m: f'<a href="{m[1]}.html">{host}/{m[1]}</a>', s)
    s = s.replace("[WEBSITE]", f'<a href="./">{host}</a>')
    s = s.replace("[DATE]", UPDATED)
    s = s.replace("[SELLER LEGAL NAME]", value(SELLER, "[SELLER LEGAL NAME]"))
    s = s.replace("[CITY]", value(CITY, "[CITY]"))
    s = s.replace("[CONTACT EMAIL]", value(EMAIL, "[CONTACT EMAIL]", f'<a href="mailto:{EMAIL}">{html.escape(EMAIL)}</a>'))
    left = set(re.findall(r"\[[A-Z][A-Z ]+\]", re.sub(r"<mark>.*?</mark>", "", s)))
    if left:
        sys.exit(f"Unknown placeholders left: {', '.join(sorted(left))}")
    return s


FOOTER = """<footer>
  <div class="wrap">
    <span>&copy; 2026 [SELLER LEGAL NAME]</span>
    <a href="download/OneClickOptimizer-Setup.exe">Download</a>
    <a href="terms.html">Terms</a>
    <a href="privacy.html">Privacy</a>
    <a href="refund.html">Refunds</a>
    <a href="licenses.html">Open-source licenses</a>
    <span>[CONTACT EMAIL]</span>
  </div>
</footer>"""

# ------------------------------------------------------------------ page

THEME_BUTTON = """<button class="theme" type="button" id="theme" aria-label="Switch to day mode" title="Day or night"><svg class="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/></svg><svg class="moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z"/></svg></button>"""

LIGHT = """    color-scheme: light;
    --bg: #f6f7f9;
    --surface: #ffffff;
    --surface-2: #eef1f4;
    --line: #e2e7ec;
    --ink: #0b0d10;
    --ink-2: #3d4753;
    --ink-3: #66727f;
    --accent: #0f766e;"""

CSS = f"""/* Generated by tools/legal-pages.py; same colours as index.html. */
:root {{
  color-scheme: dark;
  --bg: #0a0f14;
  --surface: #141b22;
  --surface-2: #1b242d;
  --line: #24303b;
  --ink: #f3f6f8;
  --ink-2: #b8c2cc;
  --ink-3: #8593a0;
  --accent: #5eead4;
  --display: "Bricolage Grotesque", "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif;
  --text: "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif;
}}
@media (prefers-color-scheme: light) {{
  :root:not([data-theme="dark"]) {{
{LIGHT}
  }}
}}
:root[data-theme="light"] {{
{LIGHT}
}}
* {{ box-sizing: border-box; }}
html {{ -webkit-text-size-adjust: 100%; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 16px/1.65 var(--text); }}
a {{ color: var(--accent); text-underline-offset: 3px; overflow-wrap: anywhere; }}
:focus-visible {{ outline: 2px solid var(--accent); outline-offset: 3px; border-radius: 6px; }}
.wrap {{ max-width: 760px; margin: 0 auto; padding: 0 16px; }}
@media (min-width: 720px) {{ .wrap {{ padding: 0 28px; }} }}
.top .wrap {{ display: flex; align-items: center; justify-content: space-between; min-height: 64px; }}
.brand {{ display: flex; align-items: center; gap: 10px; color: var(--ink); text-decoration: none; font: 700 17px/1 var(--display); }}
.brand img {{ width: 26px; height: 26px; }}
.theme {{ width: 38px; height: 38px; border-radius: 999px; border: 0; background: var(--surface-2); color: var(--ink); display: grid; place-items: center; cursor: pointer; }}
.theme svg {{ width: 18px; height: 18px; }}
.theme .sun {{ display: none; }}
:root[data-theme="light"] .theme .moon {{ display: block; }}
:root[data-theme="dark"] .theme .sun {{ display: block; }}
:root[data-theme="dark"] .theme .moon {{ display: none; }}
@media (prefers-color-scheme: light) {{ :root:not([data-theme]) .theme .moon {{ display: block; }} }}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme]) .theme .sun {{ display: block; }} :root:not([data-theme]) .theme .moon {{ display: none; }} }}
main {{ padding-top: 24px; padding-bottom: 64px; }}
h1 {{ font: 800 clamp(34px, 7vw, 52px)/1.05 var(--display); letter-spacing: -0.015em; margin: 16px 0 10px; }}
h2 {{ font: 700 21px/1.3 var(--display); margin: 40px 0 8px; }}
h2 span {{ color: var(--ink-3); font: 500 15px/1 var(--text); margin-left: 4px; }}
p, li {{ color: var(--ink-2); }}
p {{ margin: 0 0 14px; }}
p.updated {{ color: var(--ink-3); font-size: 14px; margin-bottom: 32px; }}
strong {{ color: var(--ink); }}
ol, ul {{ padding-left: 1.4em; margin: 0 0 14px; }}
li {{ margin: 6px 0; }}
ol.alpha {{ list-style: lower-alpha; }}
code {{ font-family: "Cascadia Mono", Consolas, ui-monospace, monospace; font-size: 0.88em; background: var(--surface-2); padding: 0.1em 0.4em; border-radius: 5px; color: var(--ink); }}
td code {{ white-space: nowrap; }}
.table {{ overflow-x: auto; margin: 16px 0 18px; border: 1px solid var(--line); border-radius: 14px; }}
table {{ border-collapse: collapse; width: 100%; min-width: 600px; font-size: 15px; line-height: 1.5; }}
th, td {{ text-align: left; vertical-align: top; padding: 12px 14px; border-bottom: 1px solid var(--line); }}
tbody tr:last-child td {{ border-bottom: 0; }}
th {{ color: var(--ink); background: var(--surface-2); font-weight: 600; }}
td {{ color: var(--ink-2); }}
.license {{ background: var(--surface); border: 1px solid var(--line); border-radius: 14px; padding: 16px 18px 4px; font-size: 14px; line-height: 1.6; }}
.license p {{ margin: 0 0 12px; }}
mark {{ background: #fde68a; color: #0b0d10; border-radius: 4px; padding: 0 4px; }}
footer {{ padding: 8px 0 24px; color: var(--ink-3); font-size: 14px; }}
footer .wrap {{ display: flex; flex-wrap: wrap; gap: 8px 20px; border-top: 1px solid var(--line); padding-top: 20px; }}
footer a {{ color: var(--ink-2); }}
"""

JS = """(function () {
  var root = document.documentElement;
  var btn = document.getElementById("theme");
  var meta = document.querySelector('meta[name="theme-color"]');
  function current() {
    return root.getAttribute("data-theme") || (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  }
  function sync() {
    var t = current();
    btn.setAttribute("aria-label", t === "dark" ? "Switch to day mode" : "Switch to night mode");
    if (meta) meta.setAttribute("content", t === "dark" ? "#0a0f14" : "#f6f7f9");
  }
  btn.addEventListener("click", function () {
    var t = current() === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", t);
    try { localStorage.setItem("oco-theme", t); } catch (e) {}
    sync();
  });
  window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", sync);
  sync();
  if (!/^(localhost|127\\.0\\.0\\.1|\\[::1\\])$/.test(location.hostname)) {
    var va = document.createElement("script");
    va.defer = true;
    va.src = "/_vercel/insights/script.js";
    document.head.appendChild(va);
  }
})();"""


def page(slug, title, description, body):
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · OneClick Optimizer</title>
<meta name="description" content="{description}">
<meta name="theme-color" content="#0a0f14">
<link rel="icon" href="assets/icon.png">
<link rel="canonical" href="{WEBSITE}/{slug}">
<script>
(function () {{
  try {{
    var t = localStorage.getItem("oco-theme");
    if (t === "light" || t === "dark") document.documentElement.setAttribute("data-theme", t);
  }} catch (e) {{}}
}})();
</script>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,700;12..96,800&display=swap" rel="stylesheet">
<style>
{CSS}</style>
</head>
<body>

<header class="top">
  <div class="wrap">
    <a class="brand" href="./"><img src="assets/icon.png" alt="" width="26" height="26">OneClick Optimizer</a>
    {THEME_BUTTON}
  </div>
</header>

<main class="wrap">
<h1>{title}</h1>
{body}
</main>

{FOOTER}

<script>
{JS}
</script>
</body>
</html>
"""


def read(name):
    return (ROOT / name).read_text(encoding="utf-8").replace("\r\n", "\n")


def main():
    pages = []
    _, body = markdown(read("legal/PRIVACY.md"))
    pages.append(("privacy", "Privacy policy", "What OneClick Optimizer and this website send, to whom, and why.", body))
    _, body = markdown(read("legal/REFUND.md"))
    pages.append(("refund", "Refund policy", "Refunds for OneClick Optimizer Pro.", body))
    body = eula(read("legal/EULA.txt"))
    pages.append(("terms", "Terms of use (EULA)", "The license agreement for OneClick Optimizer and OneClick Optimizer Pro.", body))
    body = notices(read("THIRD-PARTY-NOTICES.txt"))
    pages.append(("licenses", "Open-source licenses", "Open-source components in OneClick Optimizer and their licenses.", body))

    out = {f"{slug}.html": fill(page(slug, title, desc, body)) for slug, title, desc, body in pages}
    index = (DOCS / "index.html").read_text(encoding="utf-8")
    index, n = re.subn(r"<footer>.*?</footer>", lambda _: fill(FOOTER), index, flags=re.S)
    if n != 1:
        sys.exit(f"Expected one <footer> in docs/index.html, found {n}")
    out["index.html"] = index

    for name, text in out.items():
        (DOCS / name).write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote docs/{name}")
    if PREVIEW:
        print("Preview only: fill in the seller details before publishing.")


if __name__ == "__main__":
    main()
