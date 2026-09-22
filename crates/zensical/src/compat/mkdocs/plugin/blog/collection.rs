// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Stable blog collection identities, ordering, and pagination.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Stream, Value};

use crate::config::plugins::BlogPluginConfig;
use crate::path::SourcePath;

use super::PostDescriptor;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Logical type and key of a view.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum ViewKind {
    /// Main blog entrypoint.
    Blog,
    /// Formatted archive key.
    Archive(
        /// Date-derived grouping key.
        String,
    ),
    /// Normalized category identity.
    Category(
        /// Original category name.
        String,
    ),
    /// Author identifier.
    Author(
        /// Stable author identifier from the catalog.
        String,
    ),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stable identity of one configured blog instance.
#[derive(
    Clone,
    Copy,
    Debug,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct BlogId(
    /// Zero-based position of the plugin instance in configuration order.
    pub usize,
);

/// Stable identity of one post, independent of its title and route.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct PostId {
    /// Owning blog instance.
    pub blog: BlogId,
    /// Physical source identity.
    pub source: SourcePath,
}

/// Stable identity of one logical view.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ViewId {
    /// Owning blog instance.
    pub blog: BlogId,
    /// View kind and logical key.
    pub kind: ViewKind,
}

/// One post's explicit membership in one logical view.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewMembership {
    /// Containing view.
    pub view: ViewId,
    /// Display title independent of stable view identity.
    pub title: String,
    /// Configured route path relative to the blog root.
    pub path: String,
    /// Contained post.
    pub post: PostId,
    /// Pinned posts sort before unpinned posts.
    pub pin: bool,
    /// Comparable UTC creation timestamp.
    pub created: i64,
    /// Declaration position within a post for first-appearance view ordering.
    pub position: usize,
}

/// Ordering fact retained by a logical view for navigation composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewOrder {
    /// Whether the first post is pinned.
    pin: bool,
    /// UTC creation timestamp of the first post.
    created: i64,
    /// Stable identity of the first post.
    post: PostId,
    /// Declaration position for category or author memberships.
    position: usize,
}

/// Revision-complete, deterministically ordered post set for one view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedView {
    /// Logical view identity.
    pub id: ViewId,
    /// Display title independent of stable view identity.
    pub title: String,
    /// Configured route path relative to the blog root.
    pub path: String,
    /// Post identities in Material display order.
    pub posts: Arc<Vec<PostId>>,
    /// First appearance of this view in the globally ordered post sequence.
    pub order: Option<ViewOrder>,
}

/// Stable identity and boundaries of one paginated view page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewPageSpec {
    /// Logical view identity.
    pub view: ViewId,
    /// Display title independent of stable view identity.
    pub title: String,
    /// Configured route path relative to the blog root.
    pub path: String,
    /// One-based page number.
    pub page: usize,
    /// Total number of pages.
    pub pages: usize,
    /// Total number of posts across all pages.
    pub posts_total: usize,
    /// Configured maximum number of posts on one page.
    pub posts_per_page: usize,
    /// Posts shown on this page.
    pub posts: Arc<Vec<PostId>>,
    /// First appearance of the logical view in post order.
    pub order: Option<ViewOrder>,
}

/// Revision-complete logical views and their paginated projections.
pub struct Output {
    /// Ordered posts for each independent view.
    pub views: Stream<Id, OrderedView>,
    /// Stable page specifications for each view.
    pub pages: Stream<Id, ViewPageSpec>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl OrderedView {
    /// Materializes one logical view from its complete membership set.
    pub fn new(
        id: ViewId, memberships: impl IntoIterator<Item = ViewMembership>,
    ) -> Self {
        let mut memberships = memberships.into_iter().collect::<Vec<_>>();
        memberships.sort_by(display_order);
        let (title, path) = memberships
            .first()
            .map(|first| (first.title.clone(), first.path.clone()))
            .unwrap_or_default();
        debug_assert!(memberships
            .iter()
            .all(|membership| membership.path == path));
        let order = memberships.first().map(ViewOrder::from);
        Self {
            id,
            title,
            path,
            posts: Arc::new(
                memberships
                    .into_iter()
                    .map(|membership| membership.post)
                    .collect(),
            ),
            order,
        }
    }

    /// Divides this view into stable one-based page identities.
    pub fn paginate(&self, per_page: usize) -> Vec<ViewPageSpec> {
        assert!(per_page > 0, "pagination size is validated at admission");
        let pages = self.posts.len().div_ceil(per_page).max(1);
        (0..pages)
            .map(|index| {
                let start = index * per_page;
                let end = (start + per_page).min(self.posts.len());
                ViewPageSpec {
                    view: self.id.clone(),
                    title: self.title.clone(),
                    path: self.path.clone(),
                    page: index + 1,
                    pages,
                    posts_total: self.posts.len(),
                    posts_per_page: per_page,
                    posts: Arc::new(self.posts[start..end].to_vec()),
                    order: self.order.clone(),
                }
            })
            .collect()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for ViewMembership {}
impl Value for OrderedView {}
impl Value for ViewPageSpec {}

impl From<&ViewMembership> for ViewOrder {
    fn from(value: &ViewMembership) -> Self {
        Self {
            pin: value.pin,
            created: value.created,
            post: value.post.clone(),
            position: value.position,
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Installs explicit membership, per-view ordering, and pagination relations.
pub fn setup(
    posts: &Stream<Id, PostDescriptor>,
    instances: Arc<Vec<(BlogId, BlogPluginConfig)>>,
) -> Output {
    let membership_settings = instances.clone();
    let memberships = posts.flat_map(move |post: &PostDescriptor| {
        let settings = settings(&membership_settings, post.id.blog);
        Ok::<_, anyhow::Error>(
            memberships(post, settings)?
                .into_iter()
                .map(|membership| {
                    let key = membership_key(&membership.view);
                    (key, membership)
                })
                .collect::<Vec<_>>(),
        )
    });
    let views = memberships.reduce_by_key(
        |membership: &ViewMembership| view_key(&membership.view),
        |memberships: &dyn Collection<Key<Id>, ViewMembership>| {
            let mut values = memberships.values().cloned();
            let Some(first) = values.next() else {
                return Ok(None);
            };
            let id = first.view.clone();
            Ok::<_, anyhow::Error>(Some(OrderedView::new(
                id,
                std::iter::once(first).chain(values),
            )))
        },
    );
    let pages = views.clone().flat_map(move |view: &OrderedView| {
        let settings = settings(&instances, view.id.blog);
        let paginate = pagination(settings, &view.id.kind);
        let per_page = pagination_per_page(settings, &view.id.kind);
        let per_page = if paginate { per_page } else { usize::MAX };
        view.paginate(per_page)
            .into_iter()
            .map(|page| (page_key(page.page), page))
            .collect::<Vec<_>>()
    });
    Output { views, pages }
}

/// Returns whether one logical view kind is paginated.
pub fn pagination(settings: &BlogPluginConfig, kind: &ViewKind) -> bool {
    match kind {
        ViewKind::Archive(_) => {
            settings.archive_pagination.unwrap_or(settings.pagination)
        }
        ViewKind::Category(_) => settings
            .categories_pagination
            .unwrap_or(settings.pagination),
        ViewKind::Author(_) => settings
            .authors_profiles_pagination
            .unwrap_or(settings.pagination),
        ViewKind::Blog => settings.pagination,
    }
}

fn pagination_per_page(settings: &BlogPluginConfig, kind: &ViewKind) -> usize {
    match kind {
        ViewKind::Archive(_) => settings
            .archive_pagination_per_page
            .unwrap_or(settings.pagination_per_page),
        ViewKind::Category(_) => settings
            .categories_pagination_per_page
            .unwrap_or(settings.pagination_per_page),
        ViewKind::Author(_) => settings
            .authors_profiles_pagination_per_page
            .unwrap_or(settings.pagination_per_page),
        ViewKind::Blog => settings.pagination_per_page,
    }
}

fn memberships(
    post: &PostDescriptor, settings: &BlogPluginConfig,
) -> anyhow::Result<Vec<ViewMembership>> {
    let member = |kind, title, path, position| ViewMembership {
        view: ViewId { blog: post.id.blog, kind },
        title,
        path,
        post: post.id.clone(),
        pin: post.pin,
        created: post.created().timestamp_micros(),
        position,
    };
    let mut memberships =
        vec![member(ViewKind::Blog, String::new(), String::new(), 0)];
    if settings.archive {
        let key = post
            .created()
            .format_url(&settings.archive_url_date_format)?;
        let title = post
            .created()
            .format_display(&settings.archive_date_format)?;
        let path = settings.archive_url_format.replace("{date}", &key);
        memberships.push(member(ViewKind::Archive(key), title, path, 0));
    }
    if settings.categories {
        for name in &post.categories {
            let category = crate::structure::slug::unicode(
                name,
                &settings.categories_slugify_separator,
            );
            let path =
                settings.categories_url_format.replace("{slug}", &category);
            memberships.push(member(
                ViewKind::Category(name.clone()),
                name.clone(),
                path,
                0,
            ));
        }
    }
    if settings.authors_profiles {
        for (position, author) in post.authors.iter().enumerate() {
            let path = author.profile_path(settings);
            memberships.push(member(
                ViewKind::Author(author.id.clone()),
                author.name.clone(),
                path,
                position,
            ));
        }
    }
    Ok(memberships)
}

fn settings(
    instances: &[(BlogId, BlogPluginConfig)], id: BlogId,
) -> &BlogPluginConfig {
    &instances
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .expect("post refers to a configured blog instance")
        .1
}

fn membership_key(view: &ViewId) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("blog-membership")
            .resource(view.blog.0.to_string())
            .variant(view_variant(&view.kind))
            .context(".")
            .location(view_location(&view.kind))
            .build()
            .expect("validated blog view identity"),
    )
}

fn view_key(view: &ViewId) -> anyhow::Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("blog-view")
            .resource(view.blog.0.to_string())
            .variant(view_variant(&view.kind))
            .context(".")
            .location(view_location(&view.kind))
            .build()?,
    ))
}

fn page_key(page: usize) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("blog-page")
            .context(".")
            .location(page.to_string())
            .build()
            .expect("numeric page identity is valid"),
    )
}

fn view_variant(kind: &ViewKind) -> &'static str {
    match kind {
        ViewKind::Blog => "blog",
        ViewKind::Archive(_) => "archive",
        ViewKind::Category(_) => "category",
        ViewKind::Author(_) => "author",
    }
}

fn view_location(kind: &ViewKind) -> &str {
    match kind {
        ViewKind::Blog => "index",
        ViewKind::Archive(key)
        | ViewKind::Category(key)
        | ViewKind::Author(key) => key,
    }
}

/// Orders posts by pin and creation date descending, then source ascending.
fn display_order(left: &ViewMembership, right: &ViewMembership) -> Ordering {
    compare_order(&ViewOrder::from(left), &ViewOrder::from(right))
}

/// Orders views by their first appearance in Material's ordered post stream.
pub fn compare_order(left: &ViewOrder, right: &ViewOrder) -> Ordering {
    right
        .pin
        .cmp(&left.pin)
        .then_with(|| right.created.cmp(&left.created))
        .then_with(|| left.post.source.cmp(&right.post.source))
        .then_with(|| left.position.cmp(&right.position))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use zrx::id::Id;
    use zrx::stream::{Change, Key, Run, Workflow};

    use crate::compat::mkdocs::plugin::blog::{BlogDate, PostDescriptor};
    use crate::config::plugins::BlogPluginConfig;
    use crate::structure::document::DocumentHeader;
    use crate::structure::page::{PageDescriptor, PageOrigin, PageRoute};

    use super::{
        setup, BlogId, OrderedView, PostId, ViewId, ViewKind, ViewMembership,
    };

    fn membership(source: &str, created: i64, pin: bool) -> ViewMembership {
        ViewMembership {
            view: ViewId {
                blog: BlogId(0),
                kind: ViewKind::Blog,
            },
            title: String::new(),
            path: String::new(),
            post: PostId {
                blog: BlogId(0),
                source: source.parse().unwrap(),
            },
            pin,
            created,
            position: 0,
        }
    }

    #[test]
    fn orders_by_pin_date_and_deterministic_source_tie_breaker() {
        let view = OrderedView::new(
            ViewId {
                blog: BlogId(0),
                kind: ViewKind::Blog,
            },
            [
                membership("posts/b.md", 20, false),
                membership("posts/c.md", 10, true),
                membership("posts/a.md", 20, false),
                membership("posts/d.md", 30, true),
            ],
        );
        let sources = view
            .posts
            .iter()
            .map(|post| post.source.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            sources,
            ["posts/d.md", "posts/c.md", "posts/a.md", "posts/b.md"]
        );
    }

    #[test]
    fn view_order_retains_author_declaration_position() {
        let mut first = membership("zeta.md", 2, false);
        first.position = 1;
        let mut second = first.clone();
        second.position = 0;
        assert_eq!(
            super::compare_order(
                &super::ViewOrder::from(&second),
                &super::ViewOrder::from(&first),
            ),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn pagination_has_stable_one_based_ids_and_retractable_suffix() {
        let id = ViewId {
            blog: BlogId(0),
            kind: ViewKind::Category("rust".into()),
        };
        let view = OrderedView::new(
            id.clone(),
            (0..5).map(|index| {
                membership(&format!("posts/{index}.md"), index, false)
            }),
        );
        let pages = view.paginate(2);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].page, 1);
        assert_eq!(pages[2].page, 3);
        assert_eq!(pages[2].posts.len(), 1);

        let smaller = OrderedView::new(
            id,
            (0..3).map(|index| {
                membership(&format!("posts/{index}.md"), index, false)
            }),
        );
        assert_eq!(smaller.paginate(2).len(), 2);
    }

    #[test]
    fn empty_view_still_has_its_entry_page() {
        let view = OrderedView::new(
            ViewId {
                blog: BlogId(0),
                kind: ViewKind::Blog,
            },
            [],
        );
        let pages = view.paginate(10);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].posts.is_empty());
    }

    #[test]
    fn retained_graph_reorders_pages_and_retracts_obsolete_suffixes() {
        let settings = BlogPluginConfig {
            archive: false,
            categories: false,
            authors_profiles: false,
            pagination_per_page: 2,
            ..BlogPluginConfig::default()
        };
        let instances = Arc::new(vec![(BlogId(0), settings)]);
        let workflow = Workflow::<Id>::build(|workflow| {
            let posts = workflow.input::<PostDescriptor>();
            let output = setup(&posts, instances);
            workflow.output(&output.pages);
        });
        let mut runner = workflow.runner().unwrap();
        let input = runner.input::<PostDescriptor>().unwrap();

        let mut revision = input.begin().unwrap();
        for index in 0..5 {
            let source = format!("blog/posts/{index}.md");
            revision
                .insert(source_key(&source), post(&source, index))
                .unwrap();
        }
        let mut input = revision.seal().unwrap();
        let initial = changes(&mut runner.settle().unwrap());
        assert_eq!(
            initial.iter().filter(|(_, posts)| posts.is_some()).count(),
            3
        );

        let mut revision = input.begin().unwrap();
        revision.remove(source_key("blog/posts/4.md")).unwrap();
        revision.remove(source_key("blog/posts/3.md")).unwrap();
        input = revision.seal().unwrap();
        let smaller = changes(&mut runner.settle().unwrap());
        assert!(smaller
            .iter()
            .any(|(page, posts)| { page == "3" && posts.is_none() }));

        let mut revision = input.begin().unwrap();
        revision
            .insert(source_key("blog/posts/0.md"), post("blog/posts/0.md", 10))
            .unwrap();
        input = revision.seal().unwrap();
        let reordered = changes(&mut runner.settle().unwrap());
        assert!(reordered.iter().any(|(page, posts)| {
            page == "1"
                && posts.as_ref().is_some_and(|posts| {
                    posts.first().is_some_and(|source| source.ends_with("0.md"))
                })
        }));
        drop(input);
    }

    fn post(source: &str, day: i64) -> PostDescriptor {
        let source = source.parse::<crate::path::SourcePath>().unwrap();
        let document = DocumentHeader::new(
            source.clone(),
            format!("# Post {day}"),
            BTreeMap::default(),
        );
        let route = PageRoute {
            source: source.clone(),
            destination: format!("blog/{day}/index.html").parse().unwrap(),
            url: format!("blog/{day}/"),
        };
        let created =
            BlogDate::parse(&format!("2026-09-{:02}", day + 1)).unwrap();
        PostDescriptor {
            id: PostId {
                blog: BlogId(0),
                source: source.clone(),
            },
            page: PageDescriptor {
                origin: PageOrigin::Source(source),
                document,
                route,
                properties: BTreeMap::new(),
                variables: BTreeMap::new(),
            },
            dates: BTreeMap::from([("created".into(), created)]),
            author_ids: Vec::new(),
            authors: Vec::new(),
            categories: Vec::new(),
            pin: false,
            draft: None,
            slug: None,
            readtime: None,
            links: None,
        }
    }

    fn source_key(source: &str) -> Key<Id> {
        Key::from(
            Id::builder()
                .provider("test")
                .context("docs")
                .location(source)
                .build()
                .unwrap(),
        )
    }

    fn changes(run: &mut Run<Id>) -> Vec<(String, Option<Vec<String>>)> {
        let mut changes = run
            .output::<super::ViewPageSpec>()
            .unwrap()
            .map(|change| match change {
                Change::Insert(key, page) => (
                    key.iter().last().unwrap().location().to_string(),
                    Some(
                        page.posts
                            .iter()
                            .map(|post| post.source.to_string())
                            .collect(),
                    ),
                ),
                Change::Remove(key) => {
                    (key.iter().last().unwrap().location().to_string(), None)
                }
            })
            .collect::<Vec<_>>();
        changes.sort();
        changes
    }
}
