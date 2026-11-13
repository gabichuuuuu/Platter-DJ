//! The feature inventory in `docs/parity.toml` and its summary in `ROADMAP.md`.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::failure::Failure;
use crate::repo;

const INVENTORY_PATH: &str = "docs/parity.toml";
const ROADMAP_PATH: &str = "ROADMAP.md";
const SUMMARY_START: &str = "<!-- parity:start -->";
const SUMMARY_END: &str = "<!-- parity:end -->";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    area: Vec<Area>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Area {
    id: String,
    title: String,
    features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Feature {
    id: String,
    name: String,
    milestone: Milestone,
    status: Status,
    /// The test or command that demonstrates the feature.
    #[serde(default)]
    evidence: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum Milestone {
    M0,
    M1,
    M2,
    M3,
    M4,
    M5,
    /// Planned without a milestone.
    #[serde(rename = "later")]
    Later,
}

impl Milestone {
    const ALL: [Self; 7] = [
        Self::M0,
        Self::M1,
        Self::M2,
        Self::M3,
        Self::M4,
        Self::M5,
        Self::Later,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::M0 => "M0",
            Self::M1 => "M1",
            Self::M2 => "M2",
            Self::M3 => "M3",
            Self::M4 => "M4",
            Self::M5 => "M5",
            Self::Later => "Later",
        }
    }

    fn in_first_complete_version(self) -> bool {
        matches!(self, Self::M0 | Self::M1 | Self::M2 | Self::M3)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Planned,
    InProgress,
    Done,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    planned: usize,
    in_progress: usize,
    done: usize,
}

impl Counts {
    fn total(self) -> usize {
        self.planned + self.in_progress + self.done
    }

    fn row(self, label: &str) -> String {
        format!(
            "| {label} | {} | {} | {} | {} |\n",
            self.total(),
            self.done,
            self.in_progress,
            self.planned
        )
    }
}

impl Inventory {
    fn features(&self) -> impl Iterator<Item = &Feature> {
        self.area.iter().flat_map(|area| &area.features)
    }

    fn counts(&self, include: impl Fn(Milestone) -> bool) -> Counts {
        let mut counts = Counts::default();
        for feature in self.features().filter(|feature| include(feature.milestone)) {
            match feature.status {
                Status::Planned => counts.planned += 1,
                Status::InProgress => counts.in_progress += 1,
                Status::Done => counts.done += 1,
            }
        }
        counts
    }
}

/// Validates the inventory and rewrites the summary in the roadmap when it changed.
pub fn write(root: &Path) -> Result<(), Failure> {
    let (roadmap, updated) = load(root)?;
    if roadmap == updated {
        println!("parity: {ROADMAP_PATH} is up to date");
        return Ok(());
    }
    std::fs::write(root.join(ROADMAP_PATH), updated)
        .map_err(|error| Failure::message(format!("cannot write `{ROADMAP_PATH}`: {error}")))?;
    println!("parity: summary in {ROADMAP_PATH} updated");
    Ok(())
}

/// Validates the inventory and fails when the summary in the roadmap is out of date.
pub fn check(root: &Path) -> Result<(), Failure> {
    let (roadmap, updated) = load(root)?;
    if roadmap == updated {
        println!("parity: inventory valid, {ROADMAP_PATH} is up to date");
        Ok(())
    } else {
        Err(Failure::message(format!(
            "the parity summary in {ROADMAP_PATH} is out of date; run `cargo xtask parity`"
        )))
    }
}

/// The roadmap as it is and as the inventory says it should be.
fn load(root: &Path) -> Result<(String, String), Failure> {
    let inventory = parse(&repo::read_text(root, INVENTORY_PATH)?)?;
    Failure::from_findings("parity", problems(&inventory))?;
    let roadmap = repo::read_text(root, ROADMAP_PATH)?;
    let updated = with_summary(&roadmap, &summary(&inventory))?;
    Ok((roadmap, updated))
}

fn parse(text: &str) -> Result<Inventory, Failure> {
    toml::from_str(text)
        .map_err(|error| Failure::message(format!("{INVENTORY_PATH} is not valid: {error}")))
}

fn is_slug(id: &str) -> bool {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    !id.is_empty() && id.chars().all(allowed) && !id.starts_with('-') && !id.ends_with('-')
}

fn problems(inventory: &Inventory) -> Vec<String> {
    let mut problems = Vec::new();
    let mut areas = BTreeSet::new();
    for area in &inventory.area {
        if !is_slug(&area.id) {
            problems.push(format!("area id `{}` is not a lowercase slug", area.id));
        }
        if !areas.insert(area.id.as_str()) {
            problems.push(format!("area `{}` is declared twice", area.id));
        }
        if area.title.trim().is_empty() {
            problems.push(format!("area `{}` has no title", area.id));
        }
        if area.features.is_empty() {
            problems.push(format!("area `{}` has no features", area.id));
        }
        problems.extend(feature_problems(area));
    }
    problems
}

fn feature_problems(area: &Area) -> Vec<String> {
    let mut problems = Vec::new();
    let mut ids = BTreeSet::new();
    for feature in &area.features {
        let id = format!("{}.{}", area.id, feature.id);
        if !is_slug(&feature.id) {
            problems.push(format!("feature id `{id}` is not a lowercase slug"));
        }
        if !ids.insert(feature.id.as_str()) {
            problems.push(format!("feature `{id}` is declared twice"));
        }
        if feature.name.trim().is_empty() {
            problems.push(format!("feature `{id}` has no name"));
        }
        let has_evidence = !feature.evidence.trim().is_empty();
        match feature.status {
            Status::Done if !has_evidence => {
                problems.push(format!("feature `{id}` is done but names no evidence"));
            }
            Status::Planned if has_evidence => {
                problems.push(format!(
                    "feature `{id}` is planned but already names evidence"
                ));
            }
            _ => {}
        }
    }
    problems
}

fn summary(inventory: &Inventory) -> String {
    let mut text = String::from(
        "| Milestone | Features | Done | In progress | Planned |\n\
         | --- | ---: | ---: | ---: | ---: |\n",
    );
    for milestone in Milestone::ALL {
        let counts = inventory.counts(|candidate| candidate == milestone);
        if counts.total() > 0 {
            text.push_str(&counts.row(milestone.label()));
        }
    }
    text.push_str(&inventory.counts(|_| true).row("Total"));
    let first = inventory.counts(Milestone::in_first_complete_version);
    text.push_str(&format!(
        "\nFirst complete version (M0 to M3): {} of {} features done.\n",
        first.done,
        first.total()
    ));
    text
}

fn with_summary(roadmap: &str, summary: &str) -> Result<String, Failure> {
    let missing = || {
        Failure::message(format!(
            "{ROADMAP_PATH} must contain `{SUMMARY_START}` followed by `{SUMMARY_END}`"
        ))
    };
    let (head, rest) = roadmap.split_once(SUMMARY_START).ok_or_else(missing)?;
    let (_, tail) = rest.split_once(SUMMARY_END).ok_or_else(missing)?;
    Ok(format!(
        "{head}{SUMMARY_START}\n\n{summary}\n{SUMMARY_END}{tail}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{SUMMARY_END, SUMMARY_START, parse, problems, summary, with_summary};

    const SAMPLE: &str = r#"
[[area]]
id = "transport"
title = "Transport"
features = [
  { id = "play-pause", name = "Play and pause", milestone = "M1", status = "done", evidence = "cargo test -p platter-engine transport" },
  { id = "flux", name = "Flux mode", milestone = "M3", status = "planned" },
]

[[area]]
id = "effects"
title = "Effects"
features = [
  { id = "delay", name = "Delay", milestone = "M4", status = "in_progress" },
  { id = "catalogue", name = "Remaining catalogue", milestone = "later", status = "planned" },
]
"#;

    fn problems_of(text: &str) -> Vec<String> {
        problems(&parse(text).expect("the sample parses"))
    }

    #[test]
    fn accepts_a_valid_inventory() {
        assert_eq!(problems_of(SAMPLE), Vec::<String>::new());
    }

    #[test]
    fn rejects_unknown_statuses_milestones_and_fields() {
        assert!(parse(&SAMPLE.replace("in_progress", "started")).is_err());
        assert!(parse(&SAMPLE.replace("\"M4\"", "\"M9\"")).is_err());
        assert!(parse(&SAMPLE.replace("title =", "heading =")).is_err());
    }

    #[test]
    fn requires_evidence_for_done_features_only() {
        let without = SAMPLE.replace(
            ", evidence = \"cargo test -p platter-engine transport\"",
            "",
        );
        assert_eq!(problems_of(&without).len(), 1);
        let premature = SAMPLE.replace(
            "status = \"planned\" },\n]\n\n",
            "status = \"planned\", evidence = \"soon\" },\n]\n\n",
        );
        assert_eq!(problems_of(&premature).len(), 1);
    }

    #[test]
    fn rejects_duplicate_and_malformed_ids() {
        assert_eq!(
            problems_of(&SAMPLE.replace("id = \"flux\"", "id = \"play-pause\"")).len(),
            1
        );
        assert_eq!(
            problems_of(&SAMPLE.replace("id = \"flux\"", "id = \"Flux Mode\"")).len(),
            1
        );
        assert_eq!(
            problems_of(&SAMPLE.replace("id = \"effects\"", "id = \"transport\"")).len(),
            1
        );
    }

    #[test]
    fn rejects_an_area_without_features() {
        let empty = "[[area]]\nid = \"stems\"\ntitle = \"Stems\"\nfeatures = []\n";
        assert_eq!(problems_of(empty).len(), 1);
    }

    #[test]
    fn summarises_by_milestone() {
        let inventory = parse(SAMPLE).expect("the sample parses");
        let expected = "\
| Milestone | Features | Done | In progress | Planned |
| --- | ---: | ---: | ---: | ---: |
| M1 | 1 | 1 | 0 | 0 |
| M3 | 1 | 0 | 0 | 1 |
| M4 | 1 | 0 | 1 | 0 |
| Later | 1 | 0 | 0 | 1 |
| Total | 4 | 1 | 1 | 2 |

First complete version (M0 to M3): 1 of 2 features done.
";
        assert_eq!(summary(&inventory), expected);
    }

    #[test]
    fn replaces_only_the_marked_block() {
        let roadmap = format!("# Roadmap\n\n{SUMMARY_START}\nold\n{SUMMARY_END}\n\nNotes.\n");
        let updated = with_summary(&roadmap, "new\n").expect("the markers are present");
        assert_eq!(
            updated,
            format!("# Roadmap\n\n{SUMMARY_START}\n\nnew\n\n{SUMMARY_END}\n\nNotes.\n")
        );
        let again = with_summary(&updated, "new\n").expect("the markers are present");
        assert_eq!(again, updated);
    }

    #[test]
    fn requires_both_markers_in_order() {
        assert!(with_summary("# Roadmap\n", "new\n").is_err());
        let reversed = format!("{SUMMARY_END}\n{SUMMARY_START}\n");
        assert!(with_summary(&reversed, "new\n").is_err());
    }
}
