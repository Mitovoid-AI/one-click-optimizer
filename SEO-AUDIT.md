# SEO audit: oneclickoptimizer.vercel.app

Audited 22 October 2026. Site: a static, hand-written website in `docs/` (no framework), hosted on Vercel from the `main` branch with the same files mirrored on GitHub Pages.

## Summary

**SEO health score: 61 / 100.** Nothing blocks indexing: every page returns 200, HTTPS is forced, there is no `noindex`, and Lighthouse's basic SEO check scores 100. The score is held down by missing search-engine plumbing:

- no canonical tag on the home page, while an identical copy of the site is live on GitHub Pages
- no `robots.txt` and no `sitemap.xml`
- brand-only titles
- no structured data
- a render-blocking font stylesheet that delays the first paint on mobile

| Category | Weight | Score | Main reason |
|---|---|---|---|
| Technical SEO | 22% | 70 | No canonical on home, duplicate on GitHub Pages, no robots.txt or sitemap, every internal link goes through a redirect |
| Content quality | 23% | 70 | Strong, specific home page (1,289 words, measured numbers); no About/author page; legal pages are short by nature |
| On-page SEO | 20% | 55 | Titles 18 to 41 characters, legal-page descriptions 35 to 72 characters, no social tags on 4 of 5 pages, heading skip on home |
| Schema | 10% | 10 | No structured data on any page |
| Performance | 10% | 75 | Mobile Lighthouse 85: first and largest paint at 3.4 s, about 1.35 s of it blocked by the Google Fonts stylesheet; desktop 99 |
| AI search readiness | 10% | 60 | AI crawlers are allowed and the copy is concrete and citable, but no structured data and no machine-readable summary |
| Images | 5% | 70 | Good alt text and sizes, but PNG only and up to 5 times larger than displayed |

## How this was measured

- **Live HTTP checks** with `curl`: status codes, redirects, headers, `robots.txt`, `sitemap.xml`, `favicon.ico`, the 404 page, and the GitHub Pages mirror.
- **Rendered HTML** of all 5 pages, parsed for title, description, canonical, Open Graph and Twitter tags, icons, `lang`, viewport, headings, images and links.
- **Lighthouse 12.8.2**, run locally in Chrome on the live site, mobile and desktop.
- **The built-in browser** for the mobile layout and in-page anchors. Every `#anchor` link has a target, and no page scrolls sideways at 375 px.

Skipped, and why:
- **PageSpeed Insights API:** Google's keyless daily quota was used up. A free API key fixes this; see "Still to do".
- **The claude-seo scripted checks (Playwright screenshots, drift baseline, backlink lookups):** the plugin's Python runtime isn't installed. Run `/seo setup` to enable them.
- **Real-user Core Web Vitals (CrUX):** the site is too new to have field data.

## Pages

| Page | Title (chars) | Description (chars) | Canonical | Open Graph / Twitter | H1 |
|---|---|---|---|---|---|
| `/` | OneClick Optimizer (18) | 150 | missing | OG title, description, image; no `og:url`, `og:type`, Twitter | 1 |
| `/terms` | Terms of use (EULA) · OneClick Optimizer (40) | 72 | yes | none | 1 |
| `/privacy` | Privacy policy · OneClick Optimizer (35) | 64 | yes | none | 1 |
| `/refund` | Refund policy · OneClick Optimizer (34) | 35 | yes | none | 1 |
| `/licenses` | Open-source licenses · OneClick Optimizer (41) | 64 | yes | none | 1 |

## Issues

### Critical

None. Nothing blocks crawling or indexing.

### High

1. **Duplicate site without a canonical.**
   - **Evidence:** `https://mitovoid-ai.github.io/one-click-optimizer/` serves the same home page (200), and neither copy's home page has `<link rel="canonical">`.
   - **Why it matters:** Google may choose the github.io copy as the main one, and links and signals get split between the two.
   - **How we'd know it's fixed:** URL Inspection in Search Console shows the "Google-selected canonical" as `oneclickoptimizer.vercel.app/`.
2. **No `robots.txt` or `sitemap.xml`.**
   - **Evidence:** both return 404.
   - **Why it matters:** there's no sitemap to submit in Search Console, and discovery depends on links alone.
   - **How we'd know it's fixed:** Search Console reports "Sitemap: Success, 5 discovered URLs".
3. **Titles are brand-only or short.**
   - **Evidence:** the home title is "OneClick Optimizer" (18 characters). Its search result never says what the product is (a free Windows cleaner).
   - **Why it matters:** it can't match searches like "free up RAM Windows", "delete node_modules" or "PC cleaner Windows 11". The legal pages' titles are 34 to 41 characters.
4. **No structured data.**
   - **Evidence:** no JSON-LD on any page.
   - **Why it matters:** search engines and AI answers get no machine-readable facts: that it's a Windows utility, that it's free, its version and size, or who makes it.
5. **Render-blocking web font on mobile.**
   - **Evidence:** Lighthouse mobile scores 85. The first and largest paint (the intro paragraph) appear at 3.4 s, and the Google Fonts stylesheet blocks rendering for an estimated 0.9 to 1.35 s.

### Medium

6. **Short meta descriptions on the legal pages**, 35 to 72 characters. The home page's 150 characters is fine.
7. **Social sharing tags incomplete.**
   - No Open Graph tags on the 4 legal pages, and no Twitter card tags on any page.
   - `og:url`, `og:type` and `og:site_name` are missing.
   - The share image is 1710×1106 rather than the 1.91:1 shape (1200×630) that X, LinkedIn and Slack crop to.
8. **Every internal link goes through a redirect.**
   - **Evidence:** links point to `terms.html`, `privacy.html` and so on, which Vercel answers with a 308 redirect to `/terms` and `/privacy`.
   - **Why it matters:** each click costs an extra round trip, and crawlers see links to non-canonical URLs.
9. **Heading order skips a level on the home page.** The `h1` is followed directly by seven `h3` card titles in the hero carousel, before any `h2`. Lighthouse flags it as `heading-order`.
10. **Images are heavier than needed.**
    - The hero icon is a 512 px PNG (52 KB) shown at 104 px.
    - The three screenshots are 1710 px PNGs shown in a column about 550 px wide.
    - Lighthouse estimates 269 KB of savings on desktop. There are no WebP or AVIF versions.
11. **No `favicon.ico` and no Apple touch icon.**
    - `/favicon.ico` returns 404, and browsers and some crawlers request it by default.
    - The icon `<link>` has no `sizes`.

### Low

12. **Generic link text.** The seven card links all read "Open", with no label saying where they go.
13. **Small touch targets.** The carousel dots are 8 px buttons 6 px apart (Lighthouse `target-size`). Fixing this needs a small visual change.
14. **Default 404 page.** Unknown URLs get Vercel's plain 404, with no way back to the site.
15. **Basic security headers missing.** No `X-Content-Type-Options` or `Referrer-Policy`. HSTS is already on.
16. **Platform subdomain.** `vercel.app` is a shared platform domain. A custom domain builds trust and keeps the site's search history if hosting ever changes.
17. **No About or author page.** The seller's name and email are in the footer and legal pages, but nothing explains who makes the app and why. That weakens experience and trust signals, which matters for a tool that deletes files.
18. **The H1 has no product terms.** "Less junk. More memory. One click." doesn't say "Windows", "PC" or "cleaner". The intro paragraph does, so this is minor.
19. **No `llms.txt`.** Optional: some AI tools read it, while Google ignores it.

### Info (working well)

- HTTPS forced (308 from http), with HSTS preload set.
- `lang="en"` and a correct viewport on every page.
- One H1 per page; the legal pages have a clean H1 → H2 outline.
- Every meaningful image has descriptive alt text. Decorative icons have empty alt, which is correct.
- Every image has `width` and `height`, and layout shift is 0.001.
- The below-the-fold screenshots already use `loading="lazy"`.
- No `noindex`, no blocked CSS or JS, and AI crawlers are allowed (no `robots.txt` rules).
- `/eula` permanently redirects (308) to `/terms`.
- All in-page anchors resolve, and the download, checkout and Polar policy links return 200.
- Lighthouse: SEO 100 and best practices 100 on mobile and desktop; accessibility 95.
- The `_vercel/insights` cache warning comes from Vercel's analytics script, not from this site, so it isn't fixable here.

---

## Fixed

These changes are in the code (22 October 2026) and go live when you push. The site has no framework: the home page is hand-written HTML, and the legal pages are built by `tools/legal-pages.py`, so each fix was made in whichever of those owns the page. The design and layout are unchanged.

| # | Issue | Fix | Where |
|---|---|---|---|
| 1 | Duplicate site, no canonical | Self-referencing canonical on every page, pointing at `oneclickoptimizer.vercel.app`. The GitHub Pages copy now names the Vercel URL as the main one. | `docs/index.html`, `tools/legal-pages.py` |
| 2 | No robots.txt or sitemap | `docs/robots.txt` (allow all, links the sitemap) and `docs/sitemap.xml` with 5 clean URLs. Each page is dated by its last git commit, and the sitemap is rebuilt whenever the pages are. | `docs/robots.txt`, `tools/sitemap.py` |
| 3 | Weak titles | Unique titles of 55 to 59 characters on every page, e.g. "OneClick Optimizer: Free Windows PC Cleaner & RAM Optimizer". The generator refuses to build a legal page whose title or description is outside 50–60 or 140–160 characters. | same |
| 4 | No structured data | Home page: `WebSite`, `SoftwareApplication` (Windows 10 and 11, utilities, free download, screenshots) and `Person` (Ashutosh Roy) as author and publisher. Legal pages: `BreadcrumbList` (Home → page). All of it parses as valid JSON. | same |
| 5 | Render-blocking font | The Google Fonts stylesheet is preloaded and applied once it arrives, with a `<noscript>` fallback. Text shows immediately in the fallback font. | same |
| 6 | Short descriptions | Unique descriptions of 144 to 154 characters on every page. | same |
| 7 | Social tags | `og:type`, `og:site_name`, `og:url`, title, description and a new 1200×630 `og:image` (with width, height and alt) on every page. Twitter `summary_large_image` tags on every page. | same, plus `docs/assets/og.png` |
| 8 | Redirect on every internal link | All internal links use the clean URLs (`terms`, `privacy`, `refund`, `licenses`). The old `.html` URLs still redirect. | same |
| 9 | Heading skip | A screen-reader-only `h2` ("What it does, in brief") above the card deck. The outline now goes h1 → h2 → h3 with nothing visible changed. | `docs/index.html` |
| 10 | Heavy images | The icons are served at twice their display size (52 px and 208 px instead of 512 px). The screenshots are WebP with `srcset` (640, 1100 and 1710 px; tray 360 and 540 px) and `sizes`, with the PNGs as fallback, plus `decoding="async"`. Lazy loading is kept below the fold, and width and height stay set. | `docs/index.html`, `tools/site-images.py` |
| 11 | No favicon.ico | `docs/favicon.ico` (16, 32 and 48 px), `docs/apple-touch-icon.png` (180 px), and icon links with `sizes`. | same |
| 12 | "Open" link text | Each card link now has an accessible name such as "Open: Developer junk". | `docs/index.html` |
| 15 | Security headers | `X-Content-Type-Options: nosniff` and `Referrer-Policy: strict-origin-when-cross-origin` on all paths. | `docs/vercel.json` |

New tools:
- **`tools/site-images.py`** rebuilds the derived images from `docs/assets`.
- **`tools/sitemap.py`** rebuilds the sitemap.

The README's Website section now explains both.

## Verification

- **Build:** `python tools/site-images.py` then `python tools/legal-pages.py` ran cleanly. It wrote 5 pages and the sitemap, and all JSON-LD blocks parse.
- **Local server:** `serve` with clean URLs on port 8765, the same URL scheme Vercel uses.
  - The 5 pages, `robots.txt`, `sitemap.xml`, `favicon.ico`, the touch icon, the share image and every WebP return 200.
  - `/terms.html` redirects to `/terms`.
  - Every internal link and the download link returns 200.
- **Rendered `<head>` of all 5 pages:**
  - title 55 to 59 characters, description 144 to 154, one canonical each
  - complete Open Graph and Twitter tags, three icon links
  - one JSON-LD block each, one H1 each, no `.html` links left
- **In the browser:**
  - the font still loads (Bricolage Grotesque 700 and 800)
  - all 5 images load as WebP at sensible sizes (for example the 1100 px file for a 529 px slot)
  - the hero and feature sections look the same as before

### `/seo page` on the local home page, compared with the audit

The plugin's metadata script couldn't run without its Python runtime, so its checks were done by hand. The description doesn't restate the title or end on a stock call to action.

| | Audit (live, before) | Local (after) |
|---|---|---|
| Title | 18 characters, brand only | 59 characters, "Free Windows PC Cleaner & RAM Optimizer" |
| Description | 150 characters | 153 characters, rewritten for Windows 10 and 11 |
| Canonical, Open Graph, Twitter | missing or partial | complete |
| Heading order | h1 → h3 (Lighthouse fail) | h1 → h2 → h3 (Lighthouse pass) |
| Structured data | none | WebSite, SoftwareApplication, Person |
| Lighthouse mobile | performance 85, accessibility 95, SEO 100 | performance 99, accessibility 96, SEO 100 |
| Mobile largest contentful paint | 3.4 s | 1.8 s |
| Render-blocking requests | Google Fonts stylesheet, about 1.35 s | none |
| Image savings flagged (desktop) | 269 KB | 13 KB |
| Readability | not measured | Flesch 76 (easy), 1,224 words |

Local numbers come from a server on the same PC, so treat them as directional. After you push, re-run Lighthouse or PageSpeed on the live site to confirm.

Estimated page score for the home page after the fixes: **86/100**.

| Area | Score |
|---|---|
| On-page | 85 |
| Content | 78 |
| Technical | 95 |
| Schema | 75 |
| Images | 95 |

The site-wide health score should rise from 61 to about **81**. The biggest remaining gaps are content and trust signals (see below).

## Waiting for your OK

These need new visible text or a visible design change, so they're drafts only.

1. **Custom 404 page** (issue 14), as `docs/404.html`:
   - **Title:** "Page not found · OneClick Optimizer"
   - **H1:** "This page doesn't exist"
   - **Text:** "It may have moved, or the link has a typo."
   - **Links:** "Go to the home page" and "Download OneClick Optimizer (free)"
   - The usual footer, and a `noindex` tag.
2. **Product words in the H1** (issue 18). Add a small line above the headline as part of the H1: "Free Windows PC cleaner", followed by "Less junk. More memory. One click."
3. **Bigger carousel dots** (issue 13). Keep the dots the same size but space them 16 px apart instead of 6 px, and give each a 24 px invisible tap area. The dots row gets slightly wider.

## Still to do (you)

**Before or right after the next push**
1. **Push** these changes. Vercel and GitHub Pages redeploy from `main`.
2. **Google Search Console:**
   - Add the property `https://oneclickoptimizer.vercel.app/` (URL-prefix; verify with the HTML-tag or HTML-file method).
   - Submit `sitemap.xml`.
   - Run URL Inspection on `/` and check that "Google-selected canonical" is the vercel.app URL, not the github.io copy.
3. **Bing Webmaster Tools:** import the site from Search Console (one click) or add it by hand, then submit the sitemap. Bing also feeds ChatGPT search and DuckDuckGo.
4. **Re-test the live site** with PageSpeed Insights once deployed, to confirm the mobile improvement. If you want the scripted checks (PageSpeed and CrUX through the claude-seo plugin), create a free Google Cloud API key for the PageSpeed Insights API.

**Facts I need from you** (left out of the structured data rather than guessed)
5. **Publisher:** should the structured data name you personally (now: Person "Ashutosh Roy") or an organization such as "Mitovoid AI" with a logo?
6. **Pro price:** the structured data lists only the free download. To add Pro, confirm the offer: $10 lifetime until 31 January 2027, then $20. It must be updated on 1 February.
7. **Official profiles** (X, LinkedIn, YouTube, Product Hunt, GitHub) to link with `sameAs`, if any.

**Bigger wins that need your input**
8. **A custom domain** (issue 16), such as oneclickoptimizer.com. Add it in Vercel, then change `WEBSITE` in `tools/legal-pages.py` and `tools/sitemap.py` and the URLs in the `<head>` of `docs/index.html`.
9. **An About section or page** (issue 17): who you are, why you built the app, and how you test changes before they touch someone's files. Send me a few lines in your own words and I'll shape them; I won't invent your background.
10. **Content for the searches people actually make.** Each of these could be its own page, linking back to the home page:
    - "how to delete node_modules folders safely"
    - "free up RAM on Windows 11"
    - "what is VmmemWSL / Gradle daemon using memory"
11. **Backlinks and listings:**
    - Product Hunt launch, Hacker News "Show HN", r/Windows11 and r/webdev
    - AlternativeTo and Softpedia listings
    - your GitHub README linking to the site
    - a short write-up on dev.to or Hashnode about the measured results
12. **Optional:** an `llms.txt` summary for AI tools (Google ignores it).

Skipped because it needs a paid API key or account:
- PageSpeed Insights API (the free key above lifts the daily quota)
- DataForSEO (live SERP positions and keyword volumes)
- Moz and Ahrefs backlink data
