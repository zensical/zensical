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

//! Native Material blog compatibility.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, StreamSetExt, StreamTupleExt, Value};

use crate::compat::mkdocs::resource::Resource;
use crate::compat::mkdocs::{html, url};
use crate::config::plugins::{BlogPluginConfig, CategorySort};
use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::document::DocumentHeader;
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::{
    NavigationContribution, NavigationItem, NavigationResolution,
};
use crate::structure::page::{Page, PageDescriptor, PageOrigin, PageRoute};
use crate::structure::slug;
use crate::structure::toc::Section;
use crate::template::Template;
use crate::watcher::Source;

mod author;
mod collection;
mod date;
mod excerpt;
mod links;
mod pagination;
mod post;
mod readtime;

pub use author::Author;
pub use collection::{BlogId, PostId, ViewPageSpec};
pub use date::BlogDate;
pub use post::PostDescriptor;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Inputs consumed by native blog classification.
pub struct Dependencies<'a> {
    /// All resolved Markdown documents in the current revision.
    pub documents: &'a Stream<Id, DocumentHeader>,
    /// Physical sources used for watched auxiliary blog data.
    pub sources: &'a Stream<Id, Source>,
}

/// Unified page descriptors and validated posts.
pub struct Output {
    /// Ordinary pages, routed posts, and generated pages.
    pub pages: Stream<Id, PageDescriptor>,
    /// Published post descriptors for view collection.
    pub posts: Stream<Id, PostDescriptor>,
    /// Stable page specifications for populated logical views.
    pub view_pages: Stream<Id, collection::ViewPageSpec>,
    /// Revision-complete ordered logical views.
    pub views: Stream<Id, collection::OrderedView>,
}

/// Page-local variables derived from revision-complete blog views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    /// Stable identity of the page receiving this patch.
    target: Key<Id>,
    /// Rendered page content after blog-owned URL rewrites.
    pub content: Option<String>,
    /// Page fields contributed after revision-complete resolution.
    pub properties: BTreeMap<String, Dynamic>,
    /// Top-level variables consumed by Material blog templates.
    pub variables: BTreeMap<String, Dynamic>,
    /// Visible navigation URL represented by a paginated view page.
    pub navigation_url: Option<String>,
    /// Explicit hidden-page siblings, when the page isn't a visible item.
    pub siblings: Option<(Option<NavigationItem>, Option<NavigationItem>)>,
    /// View template applied after revision-complete view classification.
    pub template: Option<String>,
    /// View table of contents after optional excerpt integration.
    pub toc: Option<Vec<Section>>,
}

/// Configured native blog instances.
#[derive(Clone, Debug)]
pub struct Blog {
    /// Resolved project configuration used for routes and template values.
    config: Config,
    /// Ordered native blog instances paired with their stable identities.
    instances: Arc<Vec<(BlogId, BlogPluginConfig)>>,
    /// Whether draft-on-serve behavior is active.
    serve: bool,
    /// Build timestamp used to classify future-dated posts.
    now: i64,
}

/// One document classified as an ordinary page or blog post.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Classified {
    /// Page descriptor emitted for every admitted document.
    page: Option<PageDescriptor>,
    /// Post descriptor emitted when the document belongs to a blog.
    post: Option<PostDescriptor>,
}

/// Source or synthesized entrypoint for one configured blog.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entrypoint {
    /// Owning blog instance.
    blog: BlogId,
    /// Entrypoint document used to render the main view.
    document: DocumentHeader,
    /// Physical source when the entrypoint was supplied by the user.
    provenance: Option<SourcePath>,
}

/// Revision-complete rendered pages used by navigation composition.
#[derive(Clone, Debug)]
struct Pages(
    /// Pages in stable stream order.
    Arc<Vec<Page>>,
);

/// Revision-complete rendered pages paired with their stream identities.
#[derive(Clone, Debug)]
struct KeyedPages(
    /// Pages and the stable keys targeted by page-local patches.
    Arc<Vec<(Key<Id>, Page)>>,
);

/// Revision-complete generated view-page specifications.
#[derive(Clone, Debug)]
struct ViewPages(
    /// View pages in stable stream order.
    Arc<Vec<collection::ViewPageSpec>>,
);

/// Revision-complete document headers used by generated views.
#[derive(Clone, Debug)]
struct Documents(
    /// Documents in stable stream order.
    Arc<Vec<DocumentHeader>>,
);

/// Hidden generated page admitted into navigation relation resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
struct HiddenNavigationPage {
    /// Stable identity of the hidden page.
    target: Key<Id>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Patch {
    fn relations(
        target: Key<Id>,
        siblings: (Option<NavigationItem>, Option<NavigationItem>),
    ) -> Self {
        Self {
            target,
            content: None,
            properties: BTreeMap::new(),
            variables: BTreeMap::new(),
            navigation_url: None,
            siblings: Some(siblings),
            template: None,
            toc: None,
        }
    }

    fn merge(&mut self, other: &Self) -> anyhow::Result<()> {
        if self.target != other.target {
            anyhow::bail!("cannot merge blog patches for different pages")
        }
        merge_map(&mut self.properties, &other.properties, "properties")?;
        merge_map(&mut self.variables, &other.variables, "variables")?;
        merge_option(&mut self.content, other.content.as_ref(), "content")?;
        merge_option(
            &mut self.navigation_url,
            other.navigation_url.as_ref(),
            "navigation URL",
        )?;
        merge_option(&mut self.siblings, other.siblings.as_ref(), "siblings")?;
        merge_option(&mut self.template, other.template.as_ref(), "template")?;
        merge_option(&mut self.toc, other.toc.as_ref(), "table of contents")?;
        Ok(())
    }
}

impl Blog {
    /// Resolves enabled instances and their stable configuration-order IDs.
    pub fn new(config: &Config, serve: bool) -> Self {
        let instances = config
            .project
            .plugins
            .blogs
            .config
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.config.enabled)
            .map(|(index, instance)| (BlogId(index), instance.config.clone()))
            .collect();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
            });
        Self {
            config: config.clone(),
            instances: Arc::new(instances),
            serve,
            now,
        }
    }

    /// Routes posts before Markdown rendering and preserves ordinary pages.
    pub fn setup(&self, dependencies: Dependencies<'_>) -> Output {
        let blog = self.clone();
        let classified =
            dependencies
                .documents
                .map(move |document: &DocumentHeader| {
                    blog.classify(document.clone())
                });
        let pages = classified.filter_map(|item: &Classified| {
            item.post.is_none().then(|| item.page.clone()).flatten()
        });
        let posts =
            classified.filter_map(|item: &Classified| item.post.clone());
        let catalogs = self.author_catalogs(dependencies.sources);
        let selected = catalogs.select(&posts, |post| {
            let blog = post.id.blog;
            move |catalog: &author::Catalog| catalog.blog == blog
        });
        let blog = self.clone();
        let posts = (posts, selected).join().map(
            move |(post, catalogs): &AuthorJoin| {
                blog.resolve_authors(post.clone(), catalogs)
            },
        );
        let post_pages = posts.map(|post: &PostDescriptor| post.page.clone());
        let pages = (pages, post_pages).coalesce();
        let entrypoints = self.entrypoints(dependencies.documents);
        let collection = collection::setup(&posts, self.instances.clone());
        let views = collection.views;
        let main = views.select(&entrypoints, |entrypoint| {
            let blog = entrypoint.blog;
            move |view: &collection::OrderedView| {
                view.id.blog == blog
                    && matches!(view.id.kind, collection::ViewKind::Blog)
            }
        });
        let blog = self.clone();
        let empty = (entrypoints.clone(), main).join().filter_map(
            move |(entrypoint, views): &(
                Entrypoint,
                Vec<(Key<Id>, collection::OrderedView)>,
            )| {
                views.is_empty().then(|| collection::ViewPageSpec {
                    view: collection::ViewId {
                        blog: entrypoint.blog,
                        kind: collection::ViewKind::Blog,
                    },
                    title: entrypoint.document.title.clone(),
                    path: String::new(),
                    page: 1,
                    pages: 1,
                    posts_total: 0,
                    posts_per_page: blog
                        .settings(entrypoint.blog)
                        .pagination_per_page,
                    posts: Arc::new(Vec::new()),
                    order: None,
                })
            },
        );
        let view_pages = (collection.pages, empty).coalesce();
        let generated = self.generate_pages(
            dependencies.documents,
            &entrypoints,
            &view_pages,
        );
        let pages = (generated, pages).coalesce();
        Output {
            pages,
            posts,
            view_pages,
            views,
        }
    }

    /// Excludes hidden posts and generated blog pages from base navigation.
    pub fn navigation_pages(
        &self, pages: &Stream<Id, Page>,
        view_pages: &Stream<Id, collection::ViewPageSpec>,
    ) -> Stream<Id, Page> {
        let blog = self.clone();
        let candidates = pages.filter_map(move |page: &Page| {
            if matches!(
                page.origin(),
                PageOrigin::Generated { identity, .. }
                    if identity.starts_with("blog:")
                        && !identity.ends_with(":main:1")
            ) || blog.is_post_source(page.source())?
            {
                return Ok(None);
            }
            Ok::<_, anyhow::Error>(Some(page.clone()))
        });
        let blog = self.clone();
        let grouped = candidates.select(view_pages, move |page| {
            let source =
                (!matches!(page.view.kind, collection::ViewKind::Blog))
                    .then(|| {
                        view_page_source(blog.settings(page.view.blog), page)
                    })
                    .transpose()
                    .ok()
                    .flatten();
            move |candidate: &Page| source.as_ref() == Some(candidate.source())
        });
        let grouped = grouped
            .flat_map(|pages: &Vec<(Key<Id>, Page)>| {
                pages
                    .iter()
                    .map(|(key, _)| {
                        (
                            key.clone(),
                            HiddenNavigationPage { target: key.clone() },
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unique_by_key(|page: &HiddenNavigationPage| page.target.clone());
        (candidates, grouped).left_join().filter_map(
            |(page, hidden): &(Page, Option<HiddenNavigationPage>)| {
                hidden.is_none().then(|| page.clone())
            },
        )
    }

    /// Adds grouped views to the resolved navigation immutably.
    pub fn navigation(
        &self, resolution: &Signal<Id, NavigationResolution>,
        pages: &Stream<Id, Page>,
        view_pages: &Stream<Id, collection::ViewPageSpec>,
    ) -> Signal<Id, NavigationResolution> {
        let pages = pages.reduce(|pages: &dyn Collection<Key<Id>, Page>| {
            Some(Pages(Arc::new(pages.values().cloned().collect())))
        });
        let view_pages = view_pages.reduce(
            |pages: &dyn Collection<Key<Id>, collection::ViewPageSpec>| {
                Some(ViewPages(Arc::new(pages.values().cloned().collect())))
            },
        );
        let blog = self.clone();
        resolution
            .product(&pages)
            .product(&view_pages)
            .map(
                move |(resolution, pages): &(NavigationResolution, Pages),
                      view_pages: &ViewPages| {
                    Ok::<_, anyhow::Error>(resolution.contribute(
                        &blog.contributions(&pages.0, &view_pages.0)?,
                    ))
                },
            )
            .reduce(|values: &dyn Collection<Key<Id>, NavigationResolution>| {
                values.values().next().cloned()
            })
    }

    /// Computes a missing post read time from rendered HTML.
    pub fn apply_readtime(
        &self, source: &SourcePath, content: &str,
        properties: &mut BTreeMap<String, Dynamic>,
    ) -> anyhow::Result<()> {
        let Some((_, settings)) =
            self.instances.iter().find(|(_, settings)| {
                post::post_dir(settings).is_ok_and(|directory| {
                    source.parent().as_ref() == Some(&directory)
                        || source.is_descendant_of(&directory)
                })
            })
        else {
            return Ok(());
        };
        if !settings.post_readtime {
            return Ok(());
        }
        let Some(Dynamic::Map(config)) = properties.get_mut("config") else {
            return Ok(());
        };
        if matches!(
            config.get("readtime"),
            Some(Dynamic::Integer(value)) if *value > 0
        ) {
            return Ok(());
        }
        config.insert(
            "readtime".into(),
            Dynamic::Integer(i64::try_from(readtime::calculate(
                content,
                settings.post_readtime_words_per_minute,
            ))?),
        );
        Ok(())
    }

    /// Derives excerpt and pagination context for every rendered blog view.
    pub fn patches(
        &self, pages: &Stream<Id, Page>, posts: &Stream<Id, PostDescriptor>,
        resources: &Stream<Id, Resource>,
        view_pages: &Stream<Id, collection::ViewPageSpec>,
        ordered_views: &Stream<Id, collection::OrderedView>,
        resolution: &Signal<Id, NavigationResolution>,
    ) -> Stream<Id, Patch> {
        let patches = (
            self.view_patches(pages, resources, view_pages),
            self.post_patches(pages, posts, resources, ordered_views),
            self.relation_patches(pages, view_pages, resolution),
        )
            .coalesce();
        patches.reduce_by_key(
            |patch: &Patch| Ok::<_, anyhow::Error>(patch.target.clone()),
            |patches: &dyn Collection<Key<Id>, Patch>| {
                let mut patches = patches.values();
                let Some(first) = patches.next() else {
                    return Ok::<_, anyhow::Error>(None);
                };
                let mut merged = first.clone();
                for patch in patches {
                    merged.merge(patch)?;
                }
                Ok(Some(merged))
            },
        )
    }

    fn relation_patches(
        &self, pages: &Stream<Id, Page>,
        view_pages: &Stream<Id, collection::ViewPageSpec>,
        resolution: &Signal<Id, NavigationResolution>,
    ) -> Stream<Id, Patch> {
        let pages = pages.reduce(|pages: &dyn Collection<Key<Id>, Page>| {
            Some(KeyedPages(Arc::new(
                pages
                    .iter()
                    .map(|(key, page)| (key.clone(), page.clone()))
                    .collect(),
            )))
        });
        let view_pages = view_pages.reduce(
            |pages: &dyn Collection<Key<Id>, collection::ViewPageSpec>| {
                Some(ViewPages(Arc::new(pages.values().cloned().collect())))
            },
        );
        let blog = self.clone();
        resolution.product(&pages).product(&view_pages).flat_map(
            move |(resolution, pages): &(NavigationResolution, KeyedPages),
                  view_pages: &ViewPages| {
                blog.relation_patch_values(resolution, &pages.0, &view_pages.0)
            },
        )
    }

    fn relation_patch_values(
        &self, resolution: &NavigationResolution, pages: &[(Key<Id>, Page)],
        specs: &[collection::ViewPageSpec],
    ) -> anyhow::Result<Vec<(Key<Id>, Patch)>> {
        let values = pages
            .iter()
            .map(|(_, page)| page.clone())
            .collect::<Vec<_>>();
        let contributions = self.contributions(&values, specs)?;
        let by_url = pages
            .iter()
            .map(|(key, page)| (page.url.as_str(), (key, page)))
            .collect::<HashMap<_, _>>();
        let by_source = pages
            .iter()
            .map(|(key, page)| (page.source(), (key, page)))
            .collect::<HashMap<_, _>>();
        let mut siblings = HashMap::new();

        for contribution in contributions {
            let Some((_, host)) = by_url.get(contribution.index_url.as_str())
            else {
                continue;
            };
            let Some(visual_tail) = contribution
                .items
                .last()
                .and_then(|section| section.children.last())
                .and_then(|item| item.url.as_deref())
            else {
                continue;
            };
            let head = resolution.navigation.next_page_for_url(visual_tail);
            let mut chain = Vec::from([navigation_item(host)]);
            chain.extend(
                contribution
                    .items
                    .iter()
                    .rev()
                    .flat_map(|section| section.children.iter().cloned()),
            );
            chain.extend(head.clone());

            for (index, item) in chain.iter().enumerate() {
                let Some(url) = item.url.as_deref() else {
                    continue;
                };
                let previous = if index == 0 {
                    resolution.navigation.previous_page_for_url(url)
                } else {
                    chain.get(index - 1).cloned()
                };
                let next = if let Some(next) = chain.get(index + 1) {
                    Some(next.clone())
                } else {
                    resolution.navigation.next_page_for_url(url)
                };
                siblings.insert(url.to_owned(), (previous, next));
            }
        }

        // Material gives paginated view pages the same external siblings as
        // their first page, even though only that first page is visible in
        // navigation.
        for spec in specs.iter().filter(|spec| spec.page > 1) {
            let source = view_page_source(self.settings(spec.view.blog), spec)?;
            let mut first = spec.clone();
            first.page = 1;
            let first_source =
                view_page_source(self.settings(spec.view.blog), &first)?;
            let Some((_, first_page)) = by_source.get(&first_source) else {
                continue;
            };
            let Some(value) = siblings.get(&first_page.url).cloned() else {
                continue;
            };
            let Some((_, page)) = by_source.get(&source) else {
                continue;
            };
            siblings.insert(page.url.clone(), value);
        }

        Ok(siblings
            .into_iter()
            .filter_map(|(url, siblings)| {
                by_url.get(url.as_str()).map(|(target, _)| {
                    (
                        (*target).clone(),
                        Patch::relations((*target).clone(), siblings),
                    )
                })
            })
            .collect())
    }

    fn view_patches(
        &self, pages: &Stream<Id, Page>, resources: &Stream<Id, Resource>,
        view_pages: &Stream<Id, collection::ViewPageSpec>,
    ) -> Stream<Id, Patch> {
        let blog = self.clone();
        let views = pages.select(view_pages, move |spec| {
            let blog = blog.clone();
            let spec = spec.clone();
            move |page: &Page| blog.matches_view(page, &spec)
        });
        let selected_posts = pages.select(view_pages, |spec| {
            let sources = spec
                .posts
                .iter()
                .map(|post| post.source.clone())
                .collect::<HashSet<_>>();
            move |page: &Page| sources.contains(page.source())
        });
        let relocated_resources = resources.select(view_pages, |_| {
            |resource: &Resource| resource.source_path != resource.path
        });
        let blog = self.clone();
        let patches = (
            view_pages.clone(),
            views,
            selected_posts,
            relocated_resources,
        )
            .join()
            .filter_map(move |input: &ViewPatchInput| {
                blog.view_patch_value(input)
            });
        patches.unique_by_key(|patch: &Patch| patch.target.clone())
    }

    fn view_patch_value(
        &self, input: &ViewPatchInput,
    ) -> anyhow::Result<Option<Patch>> {
        let (spec, views, posts, resources) = input;
        let Some((target, view)) = views.first() else {
            return Ok(None);
        };
        let by_source = posts
            .iter()
            .map(|(_, page)| (page.source().clone(), page))
            .collect::<HashMap<_, _>>();
        let mappings = resource_mappings(resources);
        let excerpts = spec
            .posts
            .iter()
            .filter_map(|id| by_source.get(&id.source).copied())
            .map(|page| {
                excerpt(
                    page,
                    &view.url,
                    self.settings(spec.view.blog),
                    &mappings,
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let settings = self.settings(spec.view.blog);
        let toc = view_toc(settings, &spec.view.kind)
            .then(|| integrated_toc(view, spec, &by_source));
        let pagination = pagination_value(&self.config, settings, spec)?;
        let navigation_url = if spec.page > 1 {
            let mut first = spec.clone();
            first.page = 1;
            Some(
                PageRoute::from_source(
                    &self.config,
                    view_page_source(self.settings(spec.view.blog), &first)?,
                )?
                .url,
            )
        } else {
            None
        };
        let mut variables = BTreeMap::from([
            (
                "_blog_date_format".into(),
                Dynamic::String(settings.post_date_format.clone()),
            ),
            ("posts".into(), Dynamic::List(excerpts)),
            ("pagination".into(), pagination),
        ]);
        if let Some(url) = &navigation_url {
            variables.insert(
                "_blog_original_url".into(),
                Dynamic::String(url.clone()),
            );
        }
        Ok(Some(Patch {
            target: target.clone(),
            content: None,
            properties: BTreeMap::new(),
            navigation_url,
            siblings: None,
            template: Some("blog.html".into()),
            toc,
            variables,
        }))
    }

    fn post_patches(
        &self, pages: &Stream<Id, Page>, posts: &Stream<Id, PostDescriptor>,
        resources: &Stream<Id, Resource>,
        ordered_views: &Stream<Id, collection::OrderedView>,
    ) -> Stream<Id, Patch> {
        let main =
            ordered_views.filter_map(|view: &collection::OrderedView| {
                matches!(view.id.kind, collection::ViewKind::Blog)
                    .then(|| view.clone())
            });
        let descriptors = posts.select(&main, |view| {
            let sources = view
                .posts
                .iter()
                .map(|post| post.source.clone())
                .collect::<HashSet<_>>();
            move |post: &PostDescriptor| sources.contains(&post.id.source)
        });
        let selected = pages.select(&descriptors, |posts| {
            let mut sources = HashSet::new();
            for (_, post) in posts {
                sources.insert(post.id.source.clone());
                if let Some(items) = &post.links {
                    sources.extend(links::targets(items));
                }
            }
            move |page: &Page| sources.contains(page.source())
        });
        let selected_resources = resources.select(&descriptors, |posts| {
            let sources = posts
                .iter()
                .filter_map(|(_, post)| post.links.as_deref())
                .flat_map(links::targets)
                .collect::<HashSet<_>>();
            move |resource: &Resource| {
                resource.source_path != resource.path
                    || sources.iter().any(|source| {
                        source.as_str() == resource.source_path.as_str()
                    })
            }
        });
        let blog = self.clone();
        let patches = (main, descriptors, selected, selected_resources)
            .join()
            .flat_map(move |input: &PostPatchInput| {
                blog.post_patch_values(input)
            });
        patches.unique_by_key(|patch: &Patch| patch.target.clone())
    }

    fn post_patch_values(
        &self, input: &PostPatchInput,
    ) -> anyhow::Result<Vec<(Key<Id>, Patch)>> {
        let (view, descriptors, pages, resources) = input;
        let by_source = pages
            .iter()
            .map(|(key, page)| (page.source().clone(), (key, page)))
            .collect::<HashMap<_, _>>();
        let descriptors = descriptors
            .iter()
            .map(|(_, post)| (post.id.source.clone(), post))
            .collect::<HashMap<_, _>>();
        let resolver = links::Resolver::new(
            by_source.values().map(|(_, page)| *page),
            resources.iter().map(|(_, resource)| resource),
        );
        let mappings = resource_mappings(resources);
        let item = |index: usize| {
            view.posts.get(index).and_then(|post| {
                by_source
                    .get(&post.source)
                    .map(|(_, page)| navigation_item(page))
            })
        };
        let navigation_url = PageRoute::from_source(
            &self.config,
            entrypoint_source(self.settings(view.id.blog))?,
        )?
        .url;
        view.posts
            .iter()
            .enumerate()
            .map(|(index, post)| {
                let (target, page) = by_source
                    .get(&post.source)
                    .expect("ordered posts have selected pages");
                let content =
                    html::rewrite_urls(&page.content, &page.url, &mappings);
                let properties = descriptors
                    .get(&post.source)
                    .and_then(|post| post.links.as_deref())
                    .map(|items| resolver.resolve(items))
                    .transpose()?
                    .map_or_else(BTreeMap::new, |links| {
                        BTreeMap::from([(
                            "config".into(),
                            Dynamic::Map(BTreeMap::from([(
                                "links".into(),
                                links,
                            )])),
                        )])
                    });
                Ok((
                    (*target).clone(),
                    Patch {
                        target: (*target).clone(),
                        content,
                        properties,
                        variables: BTreeMap::new(),
                        navigation_url: Some(navigation_url.clone()),
                        siblings: Some((
                            item(index + 1),
                            index.checked_sub(1).and_then(item),
                        )),
                        template: None,
                        toc: None,
                    },
                ))
            })
            .collect()
    }

    fn classify(&self, document: DocumentHeader) -> anyhow::Result<Classified> {
        let mut matched = None;
        for (id, settings) in self.instances.iter() {
            let Some(post) = PostDescriptor::from_document(
                &self.config,
                *id,
                settings,
                document.clone(),
            )?
            else {
                continue;
            };
            if matched.is_some() {
                anyhow::bail!(
                    "post '{}' is claimed by multiple blog instances",
                    document.source
                )
            }
            matched = Some((post, settings));
        }
        if let Some((post, settings)) = matched {
            if post.is_excluded(settings, self.serve, self.now) {
                return Ok(Classified { page: None, post: None });
            }
            return Ok(Classified {
                page: Some(post.page.clone()),
                post: Some(post),
            });
        }
        let mut document = document;
        if self.entrypoint(&document.source)?.is_some() {
            document
                .meta
                .entry("template".into())
                .or_insert_with(|| Dynamic::String("blog.html".into()));
        }
        Ok(Classified {
            page: Some(PageDescriptor::source(&self.config, document)?),
            post: None,
        })
    }

    fn author_catalogs(
        &self, sources: &Stream<Id, Source>,
    ) -> Stream<Id, author::Catalog> {
        let docs = self.config.project.docs_dir.clone();
        let instances = self.instances.clone();
        sources.flat_map(move |id: &Id, source: &Source| {
            if id.context() != docs {
                return Ok(Vec::new());
            }
            let location = id.location().parse::<SourcePath>()?;
            let mut catalogs = Vec::new();
            for (blog, settings) in instances.iter() {
                if !(settings.authors || settings.authors_profiles)
                    || author::source(settings)? != location
                {
                    continue;
                }
                let data = fs::read_to_string(&**source)?;
                catalogs.push((
                    author_catalog_key(*blog),
                    author::Catalog::parse(*blog, location.clone(), &data)?,
                ));
            }
            Ok::<_, anyhow::Error>(catalogs)
        })
    }

    fn resolve_authors(
        &self, mut post: PostDescriptor,
        catalogs: &[(Key<Id>, author::Catalog)],
    ) -> anyhow::Result<PostDescriptor> {
        let settings = self.settings(post.id.blog);
        if !(settings.authors || settings.authors_profiles) {
            return Ok(post);
        }
        let catalog = match catalogs {
            [] => None,
            [(_, catalog)] => Some(catalog),
            _ => anyhow::bail!(
                "blog instance {} has multiple authors catalogs",
                post.id.blog.0
            ),
        };
        let mut authors = Vec::new();
        for id in &post.author_ids {
            let Some(author) =
                catalog.and_then(|catalog| catalog.authors.get(id))
            else {
                anyhow::bail!("couldn't find author '{id}'")
            };
            let mut author = author.clone();
            if settings.authors_profiles && author.url.is_none() {
                let source =
                    view_source(settings, &author.profile_path(settings))?;
                author.url =
                    Some(PageRoute::from_source(&self.config, source)?.url);
            }
            authors.push(author);
        }
        if settings.authors {
            post.page.properties.insert(
                "authors".into(),
                Dynamic::List(
                    authors
                        .iter()
                        .map(Dynamic::from_serialize)
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
        post.authors = authors;
        Ok(post)
    }

    fn is_post_source(&self, source: &SourcePath) -> anyhow::Result<bool> {
        for (_, settings) in self.instances.iter() {
            let directory = post::post_dir(settings)?;
            if source.parent().as_ref() == Some(&directory)
                || source.is_descendant_of(&directory)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn contributions(
        &self, pages: &[Page], specs: &[collection::ViewPageSpec],
    ) -> anyhow::Result<Vec<NavigationContribution>> {
        let template = Template::new(self.config.theme_dirs.clone());
        let mut by_source = BTreeMap::new();
        for page in pages {
            if by_source.insert(page.source().clone(), page).is_some() {
                anyhow::bail!(
                    "multiple pages expose navigation source '{}'",
                    page.source()
                )
            }
        }
        let mut contributions = Vec::new();
        for (id, settings) in self.instances.iter() {
            let mut archives = Vec::new();
            let mut categories = Vec::new();
            let mut authors = Vec::new();
            for spec in specs
                .iter()
                .filter(|spec| spec.view.blog == *id && spec.page == 1)
            {
                let source = view_page_source(settings, spec)?;
                let page = by_source.get(&source).ok_or_else(|| {
                    anyhow::anyhow!("blog view '{source}' has no rendered page")
                })?;
                match &spec.view.kind {
                    collection::ViewKind::Blog => {}
                    collection::ViewKind::Archive(_) => {
                        archives.push((
                            *page,
                            spec.order
                                .as_ref()
                                .expect("archive views have members"),
                        ));
                    }
                    collection::ViewKind::Category(name) => {
                        categories.push((name, spec.posts.len(), *page));
                    }
                    collection::ViewKind::Author(_) => {
                        authors.push((
                            *page,
                            spec.order
                                .as_ref()
                                .expect("author views have members"),
                        ));
                    }
                }
            }
            archives.sort_by(|left, right| {
                collection::compare_order(left.1, right.1)
            });
            sort_categories(settings, &mut categories);
            authors.sort_by(|left, right| {
                collection::compare_order(left.1, right.1)
            });

            let mut items = Vec::new();
            if !archives.is_empty() {
                items.push(navigation_section(
                    &template,
                    &self.config,
                    &settings.archive_name,
                    archives.into_iter().map(|(page, _)| page),
                )?);
            }
            if !categories.is_empty() {
                items.push(navigation_section(
                    &template,
                    &self.config,
                    &settings.categories_name,
                    categories.into_iter().map(|(_, _, page)| page),
                )?);
            }
            if !authors.is_empty() {
                items.push(navigation_section(
                    &template,
                    &self.config,
                    &settings.authors_profiles_name,
                    authors.into_iter().map(|(page, _)| page),
                )?);
            }
            if !items.is_empty() {
                contributions.push(NavigationContribution {
                    index_url: PageRoute::from_source(
                        &self.config,
                        entrypoint_source(settings)?,
                    )?
                    .url,
                    allow_root: settings.blog_dir.trim_matches('/') == ".",
                    items,
                });
            }
        }
        Ok(contributions)
    }

    fn entrypoints(
        &self, documents: &Stream<Id, DocumentHeader>,
    ) -> Stream<Id, Entrypoint> {
        let documents = documents.reduce(
            |documents: &dyn Collection<Key<Id>, DocumentHeader>| {
                Some(Documents(Arc::new(documents.values().cloned().collect())))
            },
        );
        let blog = self.clone();
        documents.flat_map(move |documents: &Documents| {
            let mut entrypoints = Vec::new();
            let mut sources = HashSet::new();
            for (id, settings) in blog.instances.iter() {
                let source = entrypoint_source(settings)?;
                if !sources.insert(source.clone()) {
                    anyhow::bail!(
                        "page '{source}' is claimed by multiple blog instances"
                    )
                }
                let existing = documents
                    .0
                    .iter()
                    .find(|document| document.source == source);
                let (mut document, provenance) = match existing {
                    Some(document) => {
                        (document.clone(), Some(document.source.clone()))
                    }
                    None => (
                        DocumentHeader::new(
                            source,
                            "# Blog\n\n".into(),
                            BTreeMap::new(),
                        ),
                        None,
                    ),
                };
                document
                    .meta
                    .entry("template".into())
                    .or_insert_with(|| Dynamic::String("blog.html".into()));
                entrypoints.push((
                    instance_key(*id),
                    Entrypoint {
                        blog: *id,
                        document,
                        provenance,
                    },
                ));
            }
            Ok::<_, anyhow::Error>(entrypoints)
        })
    }

    fn generate_pages(
        &self, documents: &Stream<Id, DocumentHeader>,
        entrypoints: &Stream<Id, Entrypoint>,
        pages: &Stream<Id, collection::ViewPageSpec>,
    ) -> Stream<Id, PageDescriptor> {
        let selected = entrypoints.select(pages, |page| {
            let blog = page.view.blog;
            move |entrypoint: &Entrypoint| entrypoint.blog == blog
        });
        let blog = self.clone();
        let current = documents.select(pages, move |page| {
            let source =
                view_page_source(blog.settings(page.view.blog), page).ok();
            move |document: &DocumentHeader| {
                source.as_ref() == Some(&document.source)
            }
        });
        let blog = self.clone();
        let original = documents.select(pages, move |page| {
            let mut page = page.clone();
            page.page = 1;
            let source =
                view_page_source(blog.settings(page.view.blog), &page).ok();
            move |document: &DocumentHeader| {
                source.as_ref() == Some(&document.source)
            }
        });
        let blog = self.clone();
        (pages.clone(), selected, current, original)
            .join()
            .filter_map(
                move |(page, entrypoints, current, original): &GeneratedPageInput| {
                    if page.page == 1
                        && matches!(page.view.kind, collection::ViewKind::Blog)
                        && entrypoints.first().is_some_and(|(_, entrypoint)| {
                            entrypoint.provenance.is_some()
                        })
                    {
                        return Ok(None);
                    }
                    if !current.is_empty() {
                        return Ok(None);
                    }
                    let entrypoint = entrypoints.first().map(|(_, item)| item);
                    let original =
                        original.first().map(|(_, document)| document);
                    blog.generated_view_page(page, entrypoint, original)
                        .map(Some)
                },
            )
    }

    fn generated_view_page(
        &self, page: &collection::ViewPageSpec,
        entrypoint: Option<&Entrypoint>, original: Option<&DocumentHeader>,
    ) -> anyhow::Result<PageDescriptor> {
        let settings = self.settings(page.view.blog);
        let source = view_page_source(settings, page)?;
        let (title, provenance, content) = match &page.view.kind {
            collection::ViewKind::Blog => {
                let entrypoint = entrypoint.ok_or_else(|| {
                    anyhow::anyhow!(
                        "blog instance {} has no entrypoint",
                        page.view.blog.0
                    )
                })?;
                (
                    entrypoint.document.title.clone(),
                    entrypoint.provenance.clone(),
                    entrypoint.document.body.clone(),
                )
            }
            collection::ViewKind::Archive(_)
            | collection::ViewKind::Category(_)
            | collection::ViewKind::Author(_) => {
                if let Some(original) = original {
                    (
                        original.title.clone(),
                        Some(original.source.clone()),
                        original.body.clone(),
                    )
                } else {
                    (page.title.clone(), None, format!("# {}", page.title))
                }
            }
        };
        let body = if page.page == 1 || settings.pagination_keep_content {
            content
        } else {
            format!("# {title}")
        };
        let mut meta = match (&page.view.kind, entrypoint) {
            (collection::ViewKind::Blog, Some(entrypoint)) => {
                entrypoint.document.meta.clone()
            }
            (_, _) if original.is_some() => {
                original.expect("checked above").meta.clone()
            }
            _ => BTreeMap::new(),
        };
        meta.insert("template".into(), Dynamic::String("blog.html".into()));
        let document = DocumentHeader::new(source.clone(), body, meta);
        let route = PageRoute::from_source(&self.config, source)?;
        Ok(PageDescriptor::generated(
            view_page_identity(page),
            provenance,
            document,
            route,
        ))
    }

    fn entrypoint(
        &self, source: &SourcePath,
    ) -> anyhow::Result<Option<(BlogId, &BlogPluginConfig)>> {
        let mut found = None;
        for (id, settings) in self.instances.iter() {
            if &entrypoint_source(settings)? != source {
                continue;
            }
            if found.is_some() {
                anyhow::bail!(
                    "page '{source}' is claimed by multiple blog instances"
                )
            }
            found = Some((*id, settings));
        }
        Ok(found)
    }

    fn settings(&self, id: BlogId) -> &BlogPluginConfig {
        &self
            .instances
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .expect("view refers to a configured blog instance")
            .1
    }

    fn matches_view(
        &self, page: &Page, spec: &collection::ViewPageSpec,
    ) -> bool {
        view_page_source(self.settings(spec.view.blog), spec)
            .is_ok_and(|source| page.source() == &source)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Patch {}
impl Value for Classified {}
impl Value for Entrypoint {}
impl Value for Pages {}
impl Value for KeyedPages {}
impl Value for ViewPages {}
impl Value for Documents {}
impl Value for HiddenNavigationPage {}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

type ViewPatchInput = (
    collection::ViewPageSpec,
    Vec<(Key<Id>, Page)>,
    Vec<(Key<Id>, Page)>,
    Vec<(Key<Id>, Resource)>,
);

type GeneratedPageInput = (
    collection::ViewPageSpec,
    Vec<(Key<Id>, Entrypoint)>,
    Vec<(Key<Id>, DocumentHeader)>,
    Vec<(Key<Id>, DocumentHeader)>,
);

type AuthorJoin = (PostDescriptor, Vec<(Key<Id>, author::Catalog)>);

type PostPatchInput = (
    collection::OrderedView,
    Vec<(Key<Id>, PostDescriptor)>,
    Vec<(Key<Id>, Page)>,
    Vec<(Key<Id>, Resource)>,
);

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn merge_map(
    target: &mut BTreeMap<String, Dynamic>, source: &BTreeMap<String, Dynamic>,
    field: &str,
) -> anyhow::Result<()> {
    for (key, value) in source {
        if target.get(key).is_some_and(|current| current != value) {
            anyhow::bail!("conflicting blog patch {field} key '{key}'")
        }
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn merge_option<T: Clone + PartialEq>(
    target: &mut Option<T>, source: Option<&T>, field: &str,
) -> anyhow::Result<()> {
    let Some(value) = source else {
        return Ok(());
    };
    if target.as_ref().is_some_and(|current| current != value) {
        anyhow::bail!("conflicting blog patch {field}")
    }
    *target = Some(value.clone());
    Ok(())
}

fn entrypoint_source(
    settings: &BlogPluginConfig,
) -> anyhow::Result<SourcePath> {
    let root = settings.blog_dir.trim_matches('/');
    if matches!(root, "" | ".") {
        "index.md".parse().map_err(Into::into)
    } else {
        format!("{root}/index.md").parse().map_err(Into::into)
    }
}

fn view_page_source(
    settings: &BlogPluginConfig, page: &collection::ViewPageSpec,
) -> anyhow::Result<SourcePath> {
    let source = match &page.view.kind {
        collection::ViewKind::Blog => entrypoint_source(settings)?,
        collection::ViewKind::Archive(_)
        | collection::ViewKind::Category(_)
        | collection::ViewKind::Author(_) => view_source(settings, &page.path)?,
    };
    if page.page == 1 {
        return Ok(source);
    }
    paginated_source(settings, &source, page.page)
}

fn view_source(
    settings: &BlogPluginConfig, path: &str,
) -> anyhow::Result<SourcePath> {
    let path = path.trim_matches('/');
    let root = settings.blog_dir.trim_matches('/');
    let source = if matches!(root, "" | ".") {
        format!("{path}.md")
    } else {
        format!("{root}/{path}.md")
    };
    source.parse().map_err(Into::into)
}

pub(super) fn category_source(
    settings: &BlogPluginConfig, name: &str,
) -> anyhow::Result<SourcePath> {
    let slug = slug::unicode(name, &settings.categories_slugify_separator);
    view_source(
        settings,
        &settings.categories_url_format.replace("{slug}", &slug),
    )
}

fn paginated_source(
    settings: &BlogPluginConfig, source: &SourcePath, page: usize,
) -> anyhow::Result<SourcePath> {
    let path = settings
        .pagination_url_format
        .replace("{page}", &page.to_string())
        .trim_matches('/')
        .to_owned();
    let source = source.as_str();
    let base = source
        .strip_suffix(".md")
        .expect("blog view sources use the Markdown suffix");
    let source = if base == "index" {
        format!("{path}/index.md")
    } else if let Some(parent) = base.strip_suffix("/index") {
        format!("{parent}/{path}/index.md")
    } else {
        format!("{base}/{path}.md")
    };
    source.parse().map_err(Into::into)
}

fn view_page_identity(page: &collection::ViewPageSpec) -> String {
    let kind = match &page.view.kind {
        collection::ViewKind::Blog => "main".into(),
        collection::ViewKind::Archive(key) => format!("archive:{key}"),
        collection::ViewKind::Category(key) => format!("category:{key}"),
        collection::ViewKind::Author(key) => format!("author:{key}"),
    };
    format!("blog:{}:{kind}:{}", page.view.blog.0, page.page)
}

fn instance_key(id: BlogId) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("blog-entrypoint")
            .context(".")
            .location(id.0.to_string())
            .build()
            .expect("numeric blog identity is valid"),
    )
}

fn author_catalog_key(id: BlogId) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("blog-authors")
            .context(".")
            .location(id.0.to_string())
            .build()
            .expect("numeric blog identity is valid"),
    )
}

fn navigation_section<'a>(
    template: &Template<'_>, config: &Config, title: &str,
    pages: impl IntoIterator<Item = &'a Page>,
) -> anyhow::Result<NavigationItem> {
    Ok(NavigationItem {
        title: Some(template.translate(title, config.project.as_ref())?),
        url: None,
        canonical_url: None,
        meta: None,
        children: pages.into_iter().map(navigation_item).collect(),
        is_index: false,
        active: false,
    })
}

fn sort_categories(
    settings: &BlogPluginConfig, categories: &mut [(&String, usize, &Page)],
) {
    categories.sort_by(|left, right| left.0.cmp(right.0));
    match settings.categories_sort_by {
        CategorySort::Name if settings.categories_sort_reverse => {
            categories.reverse();
        }
        CategorySort::Name => {}
        CategorySort::PostCount => categories.sort_by(|left, right| {
            let order = left.1.cmp(&right.1);
            if settings.categories_sort_reverse {
                order.reverse()
            } else {
                order
            }
        }),
    }
}

fn navigation_item(page: &Page) -> NavigationItem {
    NavigationItem {
        title: Some(page.title.clone()),
        url: Some(page.url.clone()),
        canonical_url: page.canonical_url.clone(),
        meta: Some(page.meta.clone()),
        children: Vec::new(),
        is_index: false,
        active: false,
    }
}

fn pagination_value(
    config: &Config, settings: &BlogPluginConfig,
    spec: &collection::ViewPageSpec,
) -> anyhow::Result<Dynamic> {
    if !collection::pagination(settings, &spec.view.kind) {
        return Ok(Dynamic::Null);
    }
    let empty = spec.posts_total == 0;
    let item = |page| pagination_item(config, settings, spec, page);
    let items =
        if empty || (spec.pages == 1 && !settings.pagination_if_single_page) {
            Vec::new()
        } else {
            pagination_items(config, settings, spec)?
        };
    let first_item = (spec.page - 1)
        .saturating_mul(spec.posts_per_page)
        .saturating_add(1)
        .min(spec.posts_total);
    let last_item = spec
        .page
        .saturating_mul(spec.posts_per_page)
        .min(spec.posts_total);
    let optional = |page: Option<usize>| -> anyhow::Result<Dynamic> {
        page.map(item)
            .transpose()
            .map(|item| item.unwrap_or(Dynamic::Null))
    };
    let number = |value: usize| -> anyhow::Result<Dynamic> {
        Ok(Dynamic::Integer(i64::try_from(value)?))
    };
    let boundary = |value: usize| -> anyhow::Result<Dynamic> {
        if empty {
            Ok(Dynamic::Null)
        } else {
            number(value)
        }
    };
    Ok(Dynamic::Map(BTreeMap::from([
        ("page".into(), Dynamic::Integer(i64::try_from(spec.page)?)),
        ("pages".into(), number(if empty { 0 } else { spec.pages })?),
        ("first_page".into(), boundary(1)?),
        ("last_page".into(), boundary(spec.pages)?),
        (
            "page_count".into(),
            number(if empty { 0 } else { spec.pages })?,
        ),
        (
            "items_per_page".into(),
            Dynamic::Integer(i64::try_from(spec.posts_per_page)?),
        ),
        (
            "first_item".into(),
            if empty {
                Dynamic::Null
            } else {
                number(first_item)?
            },
        ),
        (
            "last_item".into(),
            if empty {
                Dynamic::Null
            } else {
                number(last_item)?
            },
        ),
        (
            "item_count".into(),
            Dynamic::Integer(i64::try_from(spec.posts_total)?),
        ),
        ("items".into(), Dynamic::List(items)),
        ("first".into(), if empty { Dynamic::Null } else { item(1)? }),
        (
            "previous".into(),
            optional(
                (!empty)
                    .then_some(spec.page)
                    .and_then(|page| page.checked_sub(1)),
            )?,
        ),
        (
            "next".into(),
            optional(
                (!empty && spec.page < spec.pages).then_some(spec.page + 1),
            )?,
        ),
        (
            "last".into(),
            if empty {
                Dynamic::Null
            } else {
                item(spec.pages)?
            },
        ),
    ])))
}

fn pagination_items(
    config: &Config, settings: &BlogPluginConfig,
    spec: &collection::ViewPageSpec,
) -> anyhow::Result<Vec<Dynamic>> {
    let metrics = pagination::PaginationMetrics {
        page: spec.page,
        pages: spec.pages,
        items_per_page: spec.posts_per_page,
        item_count: spec.posts_total,
    };
    pagination::items(&settings.pagination_format, metrics)
        .into_iter()
        .map(|item| match item {
            pagination::PaginationItem::Page { page, current } => {
                let mut item = pagination_item(config, settings, spec, page)?;
                let Dynamic::Map(fields) = &mut item else {
                    unreachable!("pagination items are maps")
                };
                fields.insert(
                    "type".into(),
                    Dynamic::String(if current {
                        "current_page".into()
                    } else {
                        "page".into()
                    }),
                );
                Ok(item)
            }
            pagination::PaginationItem::Ellipsis => {
                Ok(pagination_static_item("span", "..", true))
            }
            pagination::PaginationItem::Link { kind, page } => {
                let mut item = pagination_item(config, settings, spec, page)?;
                let Dynamic::Map(fields) = &mut item else {
                    unreachable!("pagination items are maps")
                };
                fields.insert(
                    "type".into(),
                    Dynamic::String(kind.as_str().into()),
                );
                Ok(item)
            }
            pagination::PaginationItem::Text(value) => {
                Ok(pagination_static_item("text", &value, false))
            }
        })
        .collect()
}

fn pagination_static_item(
    item_type: &str, value: &str, ellipsis: bool,
) -> Dynamic {
    Dynamic::Map(BTreeMap::from([
        ("type".into(), Dynamic::String(item_type.into())),
        ("value".into(), Dynamic::String(value.into())),
        ("page".into(), Dynamic::Null),
        ("url".into(), Dynamic::Null),
        ("current".into(), Dynamic::Bool(false)),
        ("ellipsis".into(), Dynamic::Bool(ellipsis)),
    ]))
}

fn pagination_item(
    config: &Config, settings: &BlogPluginConfig,
    spec: &collection::ViewPageSpec, page: usize,
) -> anyhow::Result<Dynamic> {
    let mut target = spec.clone();
    target.page = page;
    let url =
        PageRoute::from_source(config, view_page_source(settings, &target)?)?
            .url;
    Ok(Dynamic::Map(BTreeMap::from([
        (
            "type".into(),
            Dynamic::String(if page == spec.page {
                "current_page".into()
            } else {
                "page".into()
            }),
        ),
        ("page".into(), Dynamic::Integer(i64::try_from(page)?)),
        ("number".into(), Dynamic::Integer(i64::try_from(page)?)),
        ("value".into(), Dynamic::String(page.to_string())),
        ("url".into(), Dynamic::String(url)),
        ("current".into(), Dynamic::Bool(page == spec.page)),
        ("ellipsis".into(), Dynamic::Bool(false)),
    ])))
}

fn view_toc(settings: &BlogPluginConfig, kind: &collection::ViewKind) -> bool {
    match kind {
        collection::ViewKind::Archive(_) => {
            settings.archive_toc.unwrap_or(settings.blog_toc)
        }
        collection::ViewKind::Category(_) => {
            settings.categories_toc.unwrap_or(settings.blog_toc)
        }
        collection::ViewKind::Author(_) => {
            settings.authors_profiles_toc.unwrap_or(settings.blog_toc)
        }
        collection::ViewKind::Blog => settings.blog_toc,
    }
}

fn integrated_toc(
    view: &Page, spec: &collection::ViewPageSpec,
    posts: &HashMap<SourcePath, &Page>,
) -> Vec<Section> {
    let mut toc = view.toc.clone();
    let Some(root) = toc.first_mut() else {
        return toc;
    };
    for post in spec.posts.iter() {
        let Some(page) = posts.get(&post.source) else {
            continue;
        };
        let mut section = page
            .toc
            .iter()
            .find(|section| section.level == 1)
            .cloned()
            .unwrap_or_else(|| Section {
                title: page.title.clone(),
                content: escape_html(&page.title),
                id: slug::ascii(&page.title, "-"),
                url: String::new(),
                children: Vec::new(),
                level: 2,
            });
        section.url = url::relative(&view.url, &page.url);
        section.children.clear();
        section.level = 2;
        root.children.push(section);
    }
    toc
}

fn resource_mappings(
    resources: &[(Key<Id>, Resource)],
) -> HashMap<String, String> {
    resources
        .iter()
        .filter(|(_, resource)| resource.source_path != resource.path)
        .map(|(_, resource)| {
            (resource.source_path.to_string(), resource.path.to_string())
        })
        .collect()
}

fn excerpt(
    page: &Page, view_url: &str, settings: &BlogPluginConfig,
    mappings: &HashMap<String, String>,
) -> anyhow::Result<Dynamic> {
    let rewritten = html::rewrite_urls(&page.content, &page.url, mappings);
    let content = rewritten.as_deref().unwrap_or(&page.content);
    let (content, more) =
        excerpt_parts(content, &settings.post_excerpt_separator);
    let href = url::relative(view_url, &page.url);
    let content = html::rebase_urls_with_fragment_base(
        content, &page.url, view_url, &href,
    )
    .unwrap_or_else(|| content.into());
    let (content, has_heading) = excerpt::headings(&content, &href);
    let content = if has_heading {
        content
    } else {
        format!(
            "<h2 id=\"{}\"><a class=\"toclink\" href=\"{href}\">{}</a></h2>\n{content}",
            slug::ascii(&page.title, "-"),
            escape_html(&page.title)
        )
    };
    let Dynamic::Map(mut value) = Dynamic::from_serialize(page)? else {
        unreachable!("pages serialize to mappings")
    };
    value.insert("content".into(), Dynamic::String(content));
    value.insert(
        "more".into(),
        more.map_or(Dynamic::Null, |more| Dynamic::String(more.into())),
    );
    truncate_list(&mut value, "authors", settings.post_excerpt_max_authors);
    truncate_list(
        &mut value,
        "categories",
        settings.post_excerpt_max_categories,
    );
    Ok(Dynamic::Map(value))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn excerpt_parts<'a>(
    content: &'a str, separator: &str,
) -> (&'a str, Option<&'a str>) {
    content
        .split_once(separator)
        .map_or((content, None), |(before, after)| (before, Some(after)))
}

fn truncate_list(
    value: &mut BTreeMap<String, Dynamic>, name: &str, maximum: usize,
) {
    if let Some(Dynamic::List(values)) = value.get_mut(name) {
        values.truncate(maximum);
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::compat::mkdocs::{html, url};

    #[test]
    fn rebases_local_excerpt_links_between_page_routes() {
        assert_eq!(
            url::rebase(
                "blog/2026/09/post/",
                "blog/page/2/",
                "../../../../notes/#detail"
            )
            .as_deref(),
            Some("../../../notes/#detail")
        );
        assert_eq!(
            html::rebase_urls(
                concat!(
                    r#"<a href="../../../../notes/#detail">Notes</a>"#,
                    r#"<img src='asset.png'>"#,
                    r#"<a href="https://example.com">External</a>"#,
                ),
                "blog/2026/09/post/",
                "blog/page/2/",
            )
            .as_deref(),
            Some(concat!(
                r#"<a href="../../../notes/#detail">Notes</a>"#,
                r#"<img src='../../2026/09/post/asset.png'>"#,
                r#"<a href="https://example.com">External</a>"#,
            ))
        );
    }
}
