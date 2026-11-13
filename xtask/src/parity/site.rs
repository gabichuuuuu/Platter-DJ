//! The public progress page: a static HTML file and a badge, generated from the inventory.
//!
//! The page loads nothing from the network: no fonts, scripts or images. Its colours are
//! the neutral tones of `docs/design/tokens.toml`.

use std::path::Path;

use super::{Counts, Inventory, Milestone, Status};
use crate::failure::Failure;

const PAGE_FILE: &str = "index.html";
const BADGE_FILE: &str = "badge.svg";

const STYLE: &str = "\
body{margin:0;padding:32px 16px;background:#111111;color:#ededed;\
font:15px/1.5 system-ui,sans-serif}\
main{max-width:880px;margin:0 auto}\
h1{font-size:28px;margin:0 0 4px}h2{font-size:18px;margin:32px 0 8px}\
p{color:#adadad;margin:0 0 16px}\
table{width:100%;border-collapse:collapse;background:#1b1b1b}\
th,td{text-align:left;padding:6px 12px;border-bottom:2px solid #111111}\
th{color:#adadad;font-weight:500}td.n,th.n{text-align:right}\
.bar{height:12px;background:#272727;margin:0 0 8px}.bar div{height:12px;background:#ededed}\
.done{color:#ededed}.in_progress{color:#adadad}.planned{color:#8a8a8a}";

/// Writes the page and the badge into `directory`, creating it if needed.
pub fn write(root: &Path, directory: &Path) -> Result<(), Failure> {
    let inventory = super::load_inventory(root)?;
    let failed = |what: &str, error: std::io::Error| {
        Failure::message(format!(
            "cannot write {what} in `{}`: {error}",
            directory.display()
        ))
    };
    std::fs::create_dir_all(directory).map_err(|error| failed("the folder", error))?;
    std::fs::write(directory.join(PAGE_FILE), page(&inventory))
        .map_err(|error| failed(PAGE_FILE, error))?;
    std::fs::write(directory.join(BADGE_FILE), badge(&inventory))
        .map_err(|error| failed(BADGE_FILE, error))?;
    println!("parity: {PAGE_FILE} and {BADGE_FILE} written");
    Ok(())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn percent(counts: Counts) -> usize {
    (counts.done * 100).checked_div(counts.total()).unwrap_or(0)
}

fn status_label(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::Done => ("done", "Done"),
        Status::InProgress => ("in_progress", "In progress"),
        Status::Planned => ("planned", "Planned"),
    }
}

fn page(inventory: &Inventory) -> String {
    let first = inventory.counts(Milestone::in_first_complete_version);
    let mut html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <title>Platter progress</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n\
         <h1>Platter progress</h1>\n\
         <p>First complete version (M0 to M3): {} of {} features done.</p>\n\
         <div class=\"bar\"><div style=\"width:{}%\"></div></div>\n\
         <h2>Milestones</h2>\n<table>\n<tr><th>Milestone</th><th class=\"n\">Features</th>\
         <th class=\"n\">Done</th><th class=\"n\">In progress</th><th class=\"n\">Planned</th></tr>\n",
        first.done,
        first.total(),
        percent(first)
    );
    for milestone in Milestone::ALL {
        let counts = inventory.counts(|candidate| candidate == milestone);
        if counts.total() > 0 {
            html.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td>\
                 <td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\n",
                milestone.label(),
                counts.total(),
                counts.done,
                counts.in_progress,
                counts.planned
            ));
        }
    }
    html.push_str("</table>\n");
    for area in &inventory.area {
        html.push_str(&format!(
            "<h2>{}</h2>\n<table>\n<tr><th>Feature</th><th>Milestone</th><th>Status</th></tr>\n",
            escape(&area.title)
        ));
        for feature in &area.features {
            let (class, label) = status_label(feature.status);
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class=\"{class}\">{label}</td></tr>\n",
                escape(&feature.name),
                feature.milestone.label()
            ));
        }
        html.push_str("</table>\n");
    }
    html.push_str("</main>\n</body>\n</html>\n");
    html
}

/// A two-part badge: the word "parity" and the percentage of the first complete version.
fn badge(inventory: &Inventory) -> String {
    let value = format!(
        "{}%",
        percent(inventory.counts(Milestone::in_first_complete_version))
    );
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"96\" height=\"20\" role=\"img\" \
         aria-label=\"parity: {value}\">\n<title>parity: {value}</title>\n\
         <rect width=\"52\" height=\"20\" fill=\"#272727\"/>\n\
         <rect x=\"52\" width=\"44\" height=\"20\" fill=\"#ededed\"/>\n\
         <g font-family=\"Verdana,sans-serif\" font-size=\"11\" text-anchor=\"middle\">\n\
         <text x=\"26\" y=\"14\" fill=\"#ededed\">parity</text>\n\
         <text x=\"74\" y=\"14\" fill=\"#111111\">{value}</text>\n</g>\n</svg>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::{badge, escape, page};
    use crate::parity::parse;

    const SAMPLE: &str = r#"
[[area]]
id = "mixer"
title = "Mixer & <master>"
features = [
  { id = "eq", name = "Three-band EQ", milestone = "M1", status = "done", evidence = "cargo test -p platter-dsp eq" },
  { id = "fx", name = "Effect \"units\"", milestone = "M4", status = "planned" },
  { id = "pan", name = "Pan", milestone = "M2", status = "in_progress" },
  { id = "sync", name = "Sync", milestone = "M3", status = "planned" },
]
"#;

    #[test]
    fn the_page_lists_every_feature_with_its_status() {
        let html = page(&parse(SAMPLE).expect("the sample parses"));
        assert!(html.contains("First complete version (M0 to M3): 1 of 3 features done."));
        assert!(html.contains("width:33%"));
        assert!(html.contains("<h2>Mixer &amp; &lt;master&gt;</h2>"));
        assert!(html.contains("<td>Three-band EQ</td><td>M1</td><td class=\"done\">Done</td>"));
        assert!(html.contains("<td>Effect &quot;units&quot;</td><td>M4</td>"));
        assert!(html.contains("<td class=\"in_progress\">In progress</td>"));
        assert!(html.contains("<tr><td>M4</td><td class=\"n\">1</td>"));
    }

    #[test]
    fn the_page_is_self_contained() {
        let html = page(&parse(SAMPLE).expect("the sample parses"));
        for external in ["http", "<script", "<link", "<img", "url("] {
            assert!(!html.contains(external), "{external}");
        }
    }

    #[test]
    fn the_badge_shows_the_percentage() {
        let svg = badge(&parse(SAMPLE).expect("the sample parses"));
        assert!(svg.starts_with("<svg xmlns="));
        assert!(svg.contains(">33%</text>"));
        assert!(svg.contains("aria-label=\"parity: 33%\""));
    }

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape("a < b & \"c\" > d"),
            "a &lt; b &amp; &quot;c&quot; &gt; d"
        );
    }
}
