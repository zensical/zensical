# Material blog oracle fixtures

These fixtures define the user-visible compatibility target for the native
Zensical blog implementation. They are built with the pinned Material checkout
and reduced to semantic JSON manifests by `scripts/blog_oracle.py`.

Run the oracle from the repository root:

```console
uv run python scripts/blog_oracle.py \
  --mkdocs ../mkdocs-material/community/venv/bin/mkdocs
```

Pass `--update` only after reviewing an intentional Material baseline change.
The harness compares routes, titles, navigation, page relations, ordered view
memberships, pagination, links, and normalized excerpt fragments. It does not
compare complete generated documents or theme assets.

The fixtures are divided by compatibility concern:

- `vertical-slice` defines the executable contract for phases 1 through 5.
- `mutations` records ordered clean-build snapshots for route, content,
  ordering, draft, deletion, and pagination changes.
- `grouped-views` covers archive, category, and author membership.
- `standalone` records a blog omitted from configured navigation.
- `multiple-instances` proves instance isolation.
- `collisions` records Material's unsafe last-writer behavior as an upstream
  quirk. Zensical is expected to diagnose the collision instead of reproducing
  it.

Additional focused fixtures are added before implementing deferred
compatibility areas such as advanced date formatting and custom templates.
