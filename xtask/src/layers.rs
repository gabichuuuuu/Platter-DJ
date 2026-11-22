//! Layer rules of the workspace and the registry of external dependencies.
//!
//! The rules are stated in `docs/architecture.md`. The checks work on a plain model of the
//! workspace, so the tests can describe one without invoking cargo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, NodeDep};

use crate::failure::Failure;
use crate::repo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    L0,
    L1,
    L2,
    L3,
    App,
    Test,
    Tool,
}

impl Layer {
    /// Position in the stack. The test kit sits beside L1: it may build on L0 only.
    fn rank(self) -> u8 {
        match self {
            Self::L0 => 0,
            Self::L1 | Self::Test => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::App => 4,
            Self::Tool => 5,
        }
    }

    /// A crate may depend on workspace crates ranked strictly below this value. The
    /// automation tool depends on none of them.
    fn ceiling(self) -> u8 {
        match self {
            Self::Tool => 0,
            other => other.rank(),
        }
    }

    fn below_l3(self) -> bool {
        matches!(self, Self::L0 | Self::L1 | Self::L2 | Self::Test)
    }
}

/// Every crate of the architecture, including those that later milestones create. A
/// workspace crate missing from this table fails the check.
const LAYER_TABLE: &[(&str, Layer)] = &[
    ("platter-core", Layer::L0),
    ("platter-dsp", Layer::L1),
    ("platter-decode", Layer::L1),
    ("platter-analysis", Layer::L1),
    ("platter-library", Layer::L1),
    ("platter-control", Layer::L1),
    ("platter-stems", Layer::L1),
    ("platter-broadcast", Layer::L1),
    ("platter-engine", Layer::L2),
    ("platter-audio-io", Layer::L3),
    ("platter-device-io", Layer::L3),
    ("platter-ui-egui", Layer::L3),
    ("platter-mcp", Layer::L3),
    ("platter", Layer::App),
    ("platter-cli", Layer::App),
    ("platter-testkit", Layer::Test),
    ("xtask", Layer::Tool),
];

/// Interface and device crates. Nothing below L3 may pull them in, directly or not.
const DEVICE_AND_UI_CRATES: &[&str] =
    &["egui", "eframe", "winit", "wgpu", "cpal", "midir", "hidapi"];

const REGISTRY_PATH: &str = "docs/dependencies.md";

/// The real-time module of the engine and the only workspace crates it may name.
const REALTIME_MODULE: &str = "crates/engine/src/rt";
const REALTIME_CRATES: &[&str] = &["platter_core", "platter_dsp"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Build,
    Dev,
}

struct Dependency {
    name: String,
    kind: Kind,
}

struct Member {
    name: String,
    /// Direct dependencies as the manifest declares them.
    dependencies: Vec<Dependency>,
    /// Every package the member pulls in: its direct dependencies of any kind and, from
    /// there on, normal and build dependencies.
    reachable: BTreeSet<String>,
}

struct Workspace {
    members: Vec<Member>,
}

impl Workspace {
    fn has_member(&self, name: &str) -> bool {
        self.members.iter().any(|member| member.name == name)
    }
}

pub fn run(root: &Path) -> Result<(), Failure> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .exec()
        .map_err(|error| Failure::message(format!("cargo metadata failed: {error}")))?;
    let workspace = workspace_from(&metadata)?;
    let registry = repo::read_text(root, REGISTRY_PATH)?;

    let mut findings = layer_violations(&workspace, LAYER_TABLE);
    findings.extend(registry_violations(
        &workspace,
        &documented_crates(&registry),
    ));
    findings.extend(realtime_violations(&realtime_sources(root)?));
    Failure::from_findings("layers", findings)?;
    println!(
        "layers: {} workspace crate(s) follow the layer rules",
        workspace.members.len()
    );
    Ok(())
}

fn workspace_from(metadata: &Metadata) -> Result<Workspace, Failure> {
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| Failure::message("cargo metadata returned no dependency graph"))?;
    let names: BTreeMap<&str, String> = metadata
        .packages
        .iter()
        .map(|package| (package.id.repr.as_str(), package.name.to_string()))
        .collect();
    let edges: BTreeMap<&str, &[NodeDep]> = resolve
        .nodes
        .iter()
        .map(|node| (node.id.repr.as_str(), node.deps.as_slice()))
        .collect();

    let members = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| Member {
            name: package.name.to_string(),
            dependencies: package
                .dependencies
                .iter()
                .map(|dependency| Dependency {
                    name: dependency.name.clone(),
                    kind: kind_of(dependency.kind),
                })
                .collect(),
            reachable: reachable(package.id.repr.as_str(), &edges, &names),
        })
        .collect();
    Ok(Workspace { members })
}

/// A kind this tool does not know is treated as a normal dependency, the strictest reading.
fn kind_of(kind: DependencyKind) -> Kind {
    match kind {
        DependencyKind::Development => Kind::Dev,
        DependencyKind::Build => Kind::Build,
        _ => Kind::Normal,
    }
}

fn reachable(
    member: &str,
    edges: &BTreeMap<&str, &[NodeDep]>,
    names: &BTreeMap<&str, String>,
) -> BTreeSet<String> {
    let direct = edges.get(member).copied().unwrap_or_default();
    let mut pending: Vec<&str> = direct.iter().map(|dep| dep.pkg.repr.as_str()).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dep in edges.get(id).copied().unwrap_or_default() {
            let shipped = dep
                .dep_kinds
                .iter()
                .any(|info| info.kind != DependencyKind::Development);
            if shipped {
                pending.push(dep.pkg.repr.as_str());
            }
        }
    }
    seen.iter()
        .filter_map(|id| names.get(id).cloned())
        .collect()
}

fn layer_violations(workspace: &Workspace, table: &[(&str, Layer)]) -> Vec<String> {
    let layer_of = |name: &str| {
        table
            .iter()
            .find(|(entry, _)| *entry == name)
            .map(|(_, layer)| *layer)
    };
    let mut violations = Vec::new();
    for member in &workspace.members {
        let Some(layer) = layer_of(&member.name) else {
            violations.push(format!("`{}` is not in the layer table", member.name));
            continue;
        };
        for dependency in &member.dependencies {
            if !workspace.has_member(&dependency.name) {
                continue;
            }
            // A dependency missing from the table is reported once, as a member.
            let Some(target) = layer_of(&dependency.name) else {
                continue;
            };
            if let Some(reason) = edge_violation(layer, target, dependency.kind) {
                violations.push(format!(
                    "`{}` ({layer:?}) depends on `{}` ({target:?}): {reason}",
                    member.name, dependency.name
                ));
            }
        }
        if layer.below_l3() {
            let pulled = DEVICE_AND_UI_CRATES
                .iter()
                .filter(|name| member.reachable.contains(**name));
            violations.extend(pulled.map(|name| {
                format!(
                    "`{}` ({layer:?}) pulls in `{name}`: interface and device crates are \
                     allowed from L3 up",
                    member.name
                )
            }));
        }
    }
    violations
}

fn edge_violation(from: Layer, to: Layer, kind: Kind) -> Option<&'static str> {
    match to {
        Layer::Tool => Some("no crate depends on the automation tool"),
        Layer::Test if kind == Kind::Dev => None,
        Layer::Test => Some("the test kit is a dev-dependency only"),
        _ if to.rank() < from.ceiling() => None,
        _ => Some("a crate depends on lower layers only"),
    }
}

/// Crate names of the registry: the first cell of each table row, written in backticks.
fn documented_crates(markdown: &str) -> BTreeSet<String> {
    markdown
        .lines()
        .filter_map(|line| line.trim().strip_prefix('|'))
        .filter_map(|row| row.split('|').next())
        .filter_map(|cell| cell.trim().strip_prefix('`')?.strip_suffix('`'))
        .map(str::to_owned)
        .collect()
}

fn registry_violations(workspace: &Workspace, documented: &BTreeSet<String>) -> Vec<String> {
    let used: BTreeSet<&str> = workspace
        .members
        .iter()
        .flat_map(|member| &member.dependencies)
        .map(|dependency| dependency.name.as_str())
        .filter(|name| !workspace.has_member(name))
        .collect();
    let missing = used
        .iter()
        .filter(|name| !documented.contains(**name))
        .map(|name| format!("dependency `{name}` has no row in {REGISTRY_PATH}"));
    let stale = documented
        .iter()
        .filter(|name| !used.contains(name.as_str()))
        .map(|name| format!("{REGISTRY_PATH} lists `{name}`, which no workspace crate uses"));
    missing.chain(stale).collect()
}

/// Source files of the real-time module, as (path relative to the repository, text). The
/// module is absent until the engine exists.
fn realtime_sources(root: &Path) -> Result<Vec<(String, String)>, Failure> {
    let directory = root.join(REALTIME_MODULE);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };
    let mut sources = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{REALTIME_MODULE}/{name}");
        if entry.path().is_dir() {
            // The check reads one level; a nested module would escape it unnoticed.
            return Err(Failure::message(format!(
                "`{relative}` is a folder: the real-time module must stay flat"
            )));
        }
        if name.ends_with(".rs") {
            sources.push((relative.clone(), repo::read_text(root, &relative)?));
        }
    }
    sources.sort();
    Ok(sources)
}

/// The real-time module may name `platter-core` and `platter-dsp` among the workspace
/// crates, and nothing of its own crate outside itself.
fn realtime_violations(sources: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    for (path, text) in sources {
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            let mut words = code
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|word| !word.is_empty())
                .peekable();
            while let Some(word) = words.next() {
                let outside = if word.starts_with("platter_") {
                    !REALTIME_CRATES.contains(&word)
                } else {
                    word == "crate" && words.peek().is_some_and(|next| *next != "rt")
                };
                if outside {
                    violations.push(format!(
                        "{path}:{}: the real-time module reaches outside itself,                          `platter-core` and `platter-dsp`",
                        number + 1
                    ));
                    break;
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        Dependency, Kind, LAYER_TABLE, Member, Workspace, documented_crates, layer_violations,
        realtime_violations, registry_violations,
    };

    fn realtime(text: &str) -> Vec<String> {
        realtime_violations(&[("crates/engine/src/rt/deck.rs".to_owned(), text.to_owned())])
    }

    #[test]
    fn the_realtime_module_may_use_core_dsp_and_itself() {
        let text = "use std::sync::Arc;
use platter_core::SampleRate;
use platter_dsp::eq::ThreeBandEq;
use rtrb::Consumer;
use super::Shared;
use crate::rt::deck::DeckPlayer;
// platter_decode is mentioned in a comment only, like crate::control.
fn crate_count() {}
";
        assert_eq!(realtime(text), Vec::<String>::new());
    }

    #[test]
    fn the_realtime_module_may_not_reach_other_crates_or_modules() {
        let found = realtime(
            "use platter_core::DeckId;
use platter_decode::decode_file;
",
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("crates/engine/src/rt/deck.rs:2:"));
        assert_eq!(
            realtime(
                "fn load() { crate::control::poll(); }
"
            )
            .len(),
            1
        );
        assert_eq!(
            realtime(
                "use crate::{control, rt};
"
            )
            .len(),
            1
        );
        assert_eq!(
            realtime(
                "let x = platter_engine::Engine::new;
"
            )
            .len(),
            1
        );
    }

    fn member(name: &str, dependencies: &[(&str, Kind)], transitive: &[&str]) -> Member {
        Member {
            name: name.to_owned(),
            dependencies: dependencies
                .iter()
                .map(|(name, kind)| Dependency {
                    name: (*name).to_owned(),
                    kind: *kind,
                })
                .collect(),
            reachable: dependencies
                .iter()
                .map(|(name, _)| *name)
                .chain(transitive.iter().copied())
                .map(str::to_owned)
                .collect(),
        }
    }

    fn violations(members: Vec<Member>) -> Vec<String> {
        layer_violations(&Workspace { members }, LAYER_TABLE)
    }

    #[test]
    fn layer_table_names_each_crate_once() {
        let names: BTreeSet<&str> = LAYER_TABLE.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), LAYER_TABLE.len());
    }

    #[test]
    fn accepts_dependencies_on_lower_layers() {
        let found = violations(vec![
            member("platter-core", &[("serde", Kind::Normal)], &[]),
            member("platter-dsp", &[("platter-core", Kind::Normal)], &[]),
            member("platter-testkit", &[("platter-core", Kind::Normal)], &[]),
            member(
                "platter-engine",
                &[
                    ("platter-core", Kind::Normal),
                    ("platter-dsp", Kind::Normal),
                    ("platter-testkit", Kind::Dev),
                ],
                &["rtrb"],
            ),
            member(
                "platter-ui-egui",
                &[("platter-engine", Kind::Normal)],
                &["egui", "wgpu"],
            ),
            member(
                "platter",
                &[("platter-ui-egui", Kind::Normal)],
                &["egui", "cpal"],
            ),
            member("xtask", &[("cargo_metadata", Kind::Normal)], &[]),
        ]);
        assert_eq!(found, Vec::<String>::new());
    }

    #[test]
    fn rejects_a_dependency_on_a_higher_layer() {
        let found = violations(vec![
            member("platter-dsp", &[("platter-engine", Kind::Normal)], &[]),
            member("platter-engine", &[], &[]),
        ]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("`platter-dsp` (L1) depends on `platter-engine` (L2)"));
    }

    #[test]
    fn rejects_a_dependency_between_crates_of_the_same_layer() {
        let found = violations(vec![
            member("platter-dsp", &[("platter-decode", Kind::Normal)], &[]),
            member("platter-decode", &[], &[]),
        ]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("lower layers only"));
    }

    #[test]
    fn rejects_a_higher_layer_even_as_dev_dependency() {
        let found = violations(vec![
            member("platter-core", &[("platter-engine", Kind::Dev)], &[]),
            member("platter-engine", &[], &[]),
        ]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn rejects_an_interface_crate_declared_below_l3() {
        let found = violations(vec![member(
            "platter-engine",
            &[("egui", Kind::Normal)],
            &[],
        )]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("`platter-engine` (L2) pulls in `egui`"));
    }

    #[test]
    fn rejects_a_device_crate_reached_through_another_dependency() {
        let found = violations(vec![member(
            "platter-decode",
            &[("symphonia", Kind::Normal)],
            &["cpal"],
        )]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("pulls in `cpal`"));
    }

    #[test]
    fn rejects_the_test_kit_as_a_normal_or_build_dependency() {
        for kind in [Kind::Normal, Kind::Build] {
            let found = violations(vec![
                member("platter-engine", &[("platter-testkit", kind)], &[]),
                member("platter-testkit", &[], &[]),
            ]);
            assert_eq!(found.len(), 1, "{kind:?}");
            assert!(found[0].contains("dev-dependency only"));
        }
    }

    #[test]
    fn rejects_the_test_kit_building_on_more_than_l0() {
        let found = violations(vec![
            member("platter-testkit", &[("platter-dsp", Kind::Normal)], &[]),
            member("platter-dsp", &[], &[]),
        ]);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn keeps_the_automation_tool_apart() {
        let found = violations(vec![
            member("xtask", &[("platter-core", Kind::Normal)], &[]),
            member("platter-core", &[("xtask", Kind::Dev)], &[]),
        ]);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn rejects_a_crate_missing_from_the_table() {
        let found = violations(vec![member("platter-surprise", &[], &[])]);
        assert_eq!(
            found,
            vec!["`platter-surprise` is not in the layer table".to_owned()]
        );
    }

    #[test]
    fn reads_crate_names_from_the_registry_table() {
        let markdown = "\
# Dependencies

| Crate | Use |
| --- | --- |
| `alpha` | parsing |
| `beta_gamma` | `inline code` in a later cell |
| not a crate | prose |
";
        let expected: BTreeSet<String> = ["alpha", "beta_gamma"].map(str::to_owned).into();
        assert_eq!(documented_crates(markdown), expected);
    }

    #[test]
    fn requires_a_registry_row_for_each_direct_dependency() {
        let workspace = Workspace {
            members: vec![
                member(
                    "platter-core",
                    &[("serde", Kind::Normal)],
                    &["serde_derive"],
                ),
                member(
                    "platter-dsp",
                    &[("platter-core", Kind::Normal), ("proptest", Kind::Dev)],
                    &[],
                ),
            ],
        };
        let documented: BTreeSet<String> = ["serde", "toml"].map(str::to_owned).into();
        let found = registry_violations(&workspace, &documented);
        assert_eq!(found.len(), 2);
        assert!(found[0].contains("`proptest` has no row"));
        assert!(found[1].contains("lists `toml`"));
    }
}
