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

The check compares generated card paths, dimensions and every card's mean RGB
pixel difference, plus each page's social metadata. A case passes when the
metadata and dimensions match and every card's mean RGB difference is at most
1 on a 0–255 scale. PNG file bytes may differ between image libraries even
when the visible output is the same. Build logs and full output are kept in a
temporary directory only while the command runs; pass `--output PATH` to keep
them for inspection.

The bundled, logo-icon and debug cases download Roboto from Google Fonts on
their first build.
Use `--font-cache PATH` to seed both temporary projects from an existing Material
font cache and run it without a network connection.

On 2026-09-23, all 10 cases matched using Material for MkDocs 9.7.1 with
Pillow 12.1.1 and this Zensical branch. The highest mean RGB difference was
0.566 (bundled layout); card paths, dimensions and social metadata matched in
every case. This is a compatibility sample, not a guarantee for every possible
layout or Material version.
