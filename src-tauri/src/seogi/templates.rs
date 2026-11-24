//! Minutes templates: what kind of recording this is and which sections its
//! minutes have. A template is data (a JSON file), so the user can change
//! the wording, add sections or make new templates without touching code.
//!
//! The built-in templates are compiled in; files in the user's templates
//! folder replace a built-in with the same `id` or add new ones.

use std::path::Path;

use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;

/// The key of the section unsettled things go to. Every template has one:
/// the pipeline moves items there when their evidence is weak.
pub const OPEN_SECTION: &str = "open";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Template {
    pub id: String,
    /// Shown to the user: `회의`, `고객 상담`.
    pub name: String,
    /// When this template fits; the classifier reads it too.
    pub description: String,
    /// What to look for in this kind of recording, added to the base rules.
    #[serde(default)]
    pub guidance: String,
    /// Whether the minutes carry feedback on the conversation itself: how
    /// the talking went, measured, and what went well and less well. On for
    /// consultations; a template's data can turn it off.
    #[serde(default)]
    pub feedback: bool,
    pub sections: Vec<Section>,
    /// The summary model for this template, when it should differ from the
    /// user's default (`ollama gemma3:27b`). Empty for the default.
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Section {
    /// Short ASCII name the model answers with: `actions`, `decisions`.
    pub key: String,
    /// The heading in the minutes: `액션 아이템`.
    pub title: String,
    pub kind: SectionKind,
    /// What belongs in this section, in the model's words.
    pub instruction: String,
    /// How the screen lays it out. Says nothing to the model.
    #[serde(default)]
    pub style: SectionStyle,
}

/// How a section of the minutes is laid out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SectionStyle {
    /// One ruled line an entry.
    #[default]
    List,
    /// A row of a table of facts, like the budget or the address. A fact
    /// the recording does not give is shown as not said, rather than left
    /// out.
    Fact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// A table of tasks: what, who, by when, source time.
    Actions,
    /// A list of statements, each with its source time.
    Items,
    /// A list of things said, quoted as they were said.
    Quotes,
}

const BUILT_IN: &[&str] = &[
    include_str!("templates/meeting.json"),
    include_str!("templates/consult.json"),
    include_str!("templates/vendor.json"),
    include_str!("templates/call.json"),
    include_str!("templates/memo.json"),
];

/// The template used when nothing says otherwise.
pub const DEFAULT_TEMPLATE: &str = "meeting";
/// The template for a recording with a single speaker.
pub const SOLO_TEMPLATE: &str = "memo";

impl Template {
    /// Makes a parsed template usable: every template ends up with an open
    /// section, and keys are unique.
    fn normalized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.sections
            .retain(|section| !section.key.trim().is_empty() && seen.insert(section.key.clone()));
        if !self
            .sections
            .iter()
            .any(|section| section.key == OPEN_SECTION)
        {
            self.sections.push(Section {
                key: OPEN_SECTION.to_string(),
                title: "아직 안 정한 것".to_string(),
                kind: SectionKind::Items,
                instruction: "결론이 나지 않았거나 모호하게 넘어간 것.".to_string(),
                style: SectionStyle::List,
            });
        }
        self
    }

    pub fn section(&self, key: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.key == key)
    }
}

/// The built-in templates.
pub fn built_in() -> Vec<Template> {
    BUILT_IN
        .iter()
        .map(|json| {
            serde_json::from_str::<Template>(json)
                .expect("built-in templates are valid")
                .normalized()
        })
        .collect()
}

/// The built-in templates with the user's own from `dir` applied over them.
/// A file that cannot be read is skipped with a warning: one bad template
/// must not stop minutes from being written.
pub fn load(dir: &Path) -> Vec<Template> {
    let mut templates = built_in();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return templates;
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    for file in files {
        match read_template(&file) {
            Ok(template) => match templates.iter_mut().find(|t| t.id == template.id) {
                Some(existing) => *existing = template,
                None => templates.push(template),
            },
            Err(e) => warn!("Skipping template {}: {e:#}", file.display()),
        }
    }
    templates
}

fn read_template(path: &Path) -> Result<Template> {
    let text = std::fs::read_to_string(path).context("could not read the file")?;
    let template: Template = serde_json::from_str(&text).context("it is not a valid template")?;
    anyhow::ensure!(!template.id.trim().is_empty(), "it has no id");
    anyhow::ensure!(!template.sections.is_empty(), "it has no sections");
    Ok(template.normalized())
}

/// Writes `template` to the user's templates folder as `<id>.json`.
pub fn save(dir: &Path, template: &Template) -> Result<()> {
    anyhow::ensure!(
        !template.id.is_empty()
            && template
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "a template id may only use letters, digits, - and _"
    );
    std::fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let json = serde_json::to_string_pretty(template)?;
    std::fs::write(dir.join(format!("{}.json", template.id)), json)
        .with_context(|| format!("could not write the template to {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_templates_parse_and_each_has_an_open_section() {
        let templates = built_in();
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["meeting", "consult", "vendor", "call", "memo"]);
        for template in &templates {
            assert!(template.section(OPEN_SECTION).is_some(), "{}", template.id);
            assert!(
                template
                    .sections
                    .iter()
                    .any(|s| s.kind == SectionKind::Actions),
                "{} has no action section",
                template.id
            );
        }
        assert!(templates.iter().any(|t| t.id == DEFAULT_TEMPLATE));
        assert!(templates.iter().any(|t| t.id == SOLO_TEMPLATE));
    }

    #[test]
    fn a_user_template_replaces_a_built_in_and_a_new_one_is_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut meeting = built_in().remove(0);
        meeting.name = "우리 회의".to_string();
        save(dir.path(), &meeting).unwrap();
        std::fs::write(
            dir.path().join("site.json"),
            r#"{"id":"site","name":"현장 점검","description":"현장을 돌며 남긴 말",
                "sections":[{"key":"actions","title":"할 일","kind":"actions","instruction":"고칠 것"}]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();

        let templates = load(dir.path());
        assert_eq!(templates.len(), 6);
        assert_eq!(templates[0].name, "우리 회의");
        let site = templates.iter().find(|t| t.id == "site").unwrap();
        // The open section is added when a template leaves it out.
        assert_eq!(site.sections.last().unwrap().key, OPEN_SECTION);
    }

    #[test]
    fn a_template_id_that_is_not_a_file_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut template = built_in().remove(0);
        template.id = "../밖".to_string();
        assert!(save(dir.path(), &template).is_err());
    }
}
