# Social plugin compatibility matrix

These projects compare the user-visible output of Material for MkDocs and
Zensical with the same `mkdocs.yml` and source files. Run the matrix from the
repository root:

```console
/Users/squidfunk/.workspace/squidfunk/repos/mkdocs-material/community/venv/bin/python \
  scripts/social_compatibility.py \
  --mkdocs /Users/squidfunk/.workspace/squidfunk/repos/mkdocs-material/community/venv/bin/mkdocs \
  --zensical .venv/bin/zensical
```

| Case | Behavior |
| --- | --- |
| `basic` | Custom layout, root and nested card routes, page title and metadata |
| `filters` | Include precedence, exclude patterns, page-level opt-out |
| `metadata` | Meta plugin inheritance and page-level layout options |
| `multiple` | Ordered plugin instances, separate card directories and metadata |
| `no-site-url` | Cards generated without public image metadata |
| `image-only` | Bundled image-only layout and a local SVG dependency |
| `bundled` | Default, accent, invert and variant layouts with typography |
| `logo-icon` | Explicit theme logo icon and omitted font settings |
| `debug` | Build-time debug grid and color settings |
| `blog` | Cards for generated blog views and posts |
| `disabled` | Disabled plugin stays disabled despite a page opt-in |
| `cards-off` | Global card switch with a page-level opt-in |
| `paths` | Flat URLs, alternate card directory and custom layout directory |
| `layout-options` | Bundled-layout image, logo, title, description and page overrides |
| `layer-composition` | YAML definitions, SVG background, icon, origins and offsets |
| `theme-defaults` | Palette list, SVG/PNG logos, exact font face and variant fallback |
| `custom-typography` | Custom alignment, wrapping, shrinking and line spacing |
| `template-context` | Page file and URL values inside custom tag templates |
| `lifecycle` | Cached rebuilds after image, page metadata and layout edits |
| `recoverable-error` | Missing image with ignored card-generation error |
| `deprecated` | Deprecated `cards_color` and `cards_font` settings |
| `debug-no-grid` | Debug overlay without a grid |

The check compares generated card paths, dimensions and every card's mean RGB
pixel difference, plus each page's social metadata. A case passes when the
metadata and dimensions match and every card's mean RGB difference is at most
1 on a 0–255 scale. The `lifecycle` case also rebuilds both engines after each
exact source edit and checks that the expected output changed. PNG file bytes
may differ between image libraries even when the visible output is the same.
Build logs and full output are kept in a temporary directory only while the
command runs; pass `--output PATH` to keep them for inspection.

The cases using typography or debug labels download Roboto from Google Fonts
on their first build. Use `--font-cache PATH` to seed both temporary projects
from an existing Material font cache and run without a network connection.

On 2026-09-23, 19 of 22 cases passed the strict pixel threshold using Material
for MkDocs 9.7.1 with Pillow 12.1.1 and this Zensical branch. One remaining
case is an accepted rasterization difference; the other two are behavior gaps:

- `custom-typography`: text placement matches visually. Different text
  rasterizers exceed the strict pixel threshold (maximum mean RGB difference
  3.636), but this is not considered a user-visible defect.
- `template-context`: Material exposes `page.file.src_uri` to layout templates;
  the Zensical build fails because that value is undefined.
- `lifecycle`: after a cached SVG background edit, Material retains its old
  card while Zensical updates it. The initial build and later layout edit match.

The full matrix currently exits nonzero because of the strict pixel threshold
and the two behavior gaps.
It does not prove every custom layout, live `serve` edit, remote font, or error
policy combination. Configuration keys are represented across this matrix and
the Python integration tests, but a finite set of cases cannot prove universal
behavioral parity.
