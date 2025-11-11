// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

//! MiniJinja template engine.

use minijinja::{context, AutoEscape, Environment, Error};
use minijinja_contrib::filters::striptags;
use serde::Serialize;
use std::path::PathBuf;

use super::config::Config;
use super::structure::nav::Navigation;

mod filter;
mod loader;

use filter::{script_tag_filter, url_filter};
use loader::Loader;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MiniJinja template.
pub struct Template<'a> {
    /// Template environment
    env: Environment<'a>,
    /// Template name.
    name: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Template<'_> {
    /// Creates a template.
    pub fn new<S, D>(name: S, dirs: D) -> Self
    where
        S: Into<String>,
        D: IntoIterator<Item = PathBuf>,
    {
        let mut env = Environment::new();

        // Create template loader with support for theme overrides
        let loader = Loader::new(dirs);
        env.set_loader(move |name| loader.load(name));

        // Register the striptags filter, which isn't part of MiniJinja's common
        // filters, and add our custom filters to replicate MkDocs' behavior
        env.add_filter("striptags", striptags);
        env.add_filter("url", url_filter);
        env.add_filter("script_tag", script_tag_filter);

        // Reset auto-escaping, as we don't want to escape HTML in templates
        env.set_auto_escape_callback(|_| AutoEscape::None);
        Self { env, name: name.into() }
    }

    /// Renders the template with the given context.
    pub fn render_with_context<C>(&self, context: C) -> Result<String, Error>
    where
        C: Serialize,
    {
        let template = self.env.get_template(&self.name)?;
        template.render(context)
    }

    /// Renders the template.
    pub fn render(
        &self, config: &Config, nav: &Navigation,
    ) -> Result<String, Error> {
        let template = self.env.get_template(&self.name)?;
        let pages = nav.iter().collect::<Vec<_>>();

        // Create context and render template
        template.render(context! {
            generator => GENERATOR,
            nav => nav,
            pages => pages,
            base_url => config.get_base_path(),
            extra_css => config.project.extra_css.clone(),
            extra_javascript => config.project.extra_javascript.clone(),
            config => config.project.clone(),
            // MiniJinja does not allow to pass empty objects, so we create a
            // dummy page here - these won't be used in static templates
            page => context! {
                ancestors => Vec::<()>::new(),
                toc => Vec::<()>::new()
            },
        })
    }
}

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Generator string.
pub const GENERATOR: &str =
    concat!(env!("CARGO_PKG_NAME"), "-", env!("CARGO_PKG_VERSION"));
