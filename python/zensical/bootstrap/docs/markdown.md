---
icon: simple/markdown
---

# Intro to Markdown

!!! warning ""
    Zensical uses [Python Markdown] to be compatible with [Material for MkDocs].
    In the medium term, we are considering adding support or CommonMark and
    providing tools for existing projects to migrate. Check out our [roadmap].

[Python Markdown]: https://python-markdown.github.io/
[Material for MkDocs]: https://squidfunk.github.io/mkdocs-material/
[CommonMark]: https://commonmark.org/
[roadmap]: https://zensical.org/about/roadmap/

Text in Markdown can be _italicized_, __bold face__.

Markdown allows you to produce bullet point lists:

* bullet
* point
* list
    * nested
    * list

as well as numbered lists:

1. numbered
2. list
    1. nested
    2. list

> If you can't explain it to a six year old, you don't understand it
> yourself.<br> (Albert Einstein)


| Feature        | Supported | Notes                      |
| -------------- | --------- | -------------------------- |
| Admonitions    | ✅         | Native support            |
| Code Highlight | ✅         | Pygments & Superfences    |
| Task Lists     | ✅         | Pymdown extensions        |
| Emojis         | ✅         | GitHub-style emoji        |


<figure markdown="span">
  ![Image title](https://dummyimage.com/600x400/){ width="300" }
  <figcaption>Image caption</figcaption>
</figure>
