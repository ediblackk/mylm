//! Template engine for synthesizing multiple LLM responses.
//!
//! Uses Tera templates to combine multiple reference responses into
//! a single synthesized answer, resolving contradictions and merging insights.
//!
//! # Main Types
//! - `TemplateEngine`: Tera-based template renderer

use std::sync::Arc;
use tera::{Context, Tera};
use crate::pacore::error::Error;

#[derive(Clone, Default)]
pub struct TemplateEngine {
    tera: Arc<Tera>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        let mut tera = Tera::default();
        tera.add_raw_template("synthesis_prompt", SYNTHESIS_TEMPLATE).expect("Failed to register synthesis template");
        Self { tera: Arc::new(tera) }
    }

    pub fn render(&self, template_name: &str, original_content: &str, ref_responses: &[String]) -> Result<String, Error> {
        let mut context = Context::new();
        context.insert("original_content", original_content);
        context.insert("ref_responses", ref_responses);

        self.tera.render(template_name, &context)
            .map_err(|e| Error::Template(e.to_string()))
    }
}

const SYNTHESIS_TEMPLATE: &str = r#"
You are given a problem and {{ ref_responses|length }} reference responses.
Some responses may contain errors, some may be incomplete.

Problem:
{{ original_content }}

Reference responses:
{% for resp in ref_responses %}
---
Response {{ loop.index }}:
{{ resp }}
{% endfor %}

Task: Synthesize a correct and complete answer. Incorporate correct insights
from any responses and resolve contradictions. Return only the final answer.
"#;
