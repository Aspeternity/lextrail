//! Small helpers over `scraper` elements: classes, text with skipped parts, scoped search.

use ego_tree::NodeRef;
use scraper::{ElementRef, Node};

/// Elements whose text is never part of a field: search index copies, list bullets,
/// sound icons, empty placeholders.
const NEVER_TEXT: &[&str] = &[
    "t-ftindex",
    "t-fthzindex",
    "t-infl",
    "t-xsymb",
    "t-sdsymb",
    "t-drtri",
    "t-symbol",
    "t-audio-gb",
    "t-audio-us",
    "t-audio-afr",
    "t-arrow",
];

pub fn has_class(el: ElementRef, class: &str) -> bool {
    el.value().classes().any(|c| c == class)
}

pub fn has_any_class(el: ElementRef, classes: &[&str]) -> bool {
    el.value().classes().any(|c| classes.contains(&c))
}

/// Element children of `el`.
pub fn child_elements(el: ElementRef<'_>) -> impl Iterator<Item = ElementRef<'_>> {
    el.children().filter_map(ElementRef::wrap)
}

/// The first element child with the class.
pub fn child<'a>(el: ElementRef<'a>, class: &str) -> Option<ElementRef<'a>> {
    child_elements(el).find(|c| has_class(*c, class))
}

/// Descendants of `root` (not `root` itself) that `matches`, in document order, without
/// looking inside elements that match or that are a `barrier`.
pub fn scoped<'a>(
    root: ElementRef<'a>,
    matches: &dyn Fn(ElementRef) -> bool,
    barrier: &dyn Fn(ElementRef) -> bool,
) -> Vec<ElementRef<'a>> {
    fn walk<'a>(
        el: ElementRef<'a>,
        matches: &dyn Fn(ElementRef) -> bool,
        barrier: &dyn Fn(ElementRef) -> bool,
        out: &mut Vec<ElementRef<'a>>,
    ) {
        for c in child_elements(el) {
            if matches(c) {
                out.push(c);
            } else if !barrier(c) {
                walk(c, matches, barrier, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, matches, barrier, &mut out);
    out
}

/// The first descendant with the class, not looking inside a `barrier`.
pub fn find<'a>(
    root: ElementRef<'a>,
    class: &str,
    barrier: &dyn Fn(ElementRef) -> bool,
) -> Option<ElementRef<'a>> {
    scoped(root, &|e| has_class(e, class), barrier)
        .into_iter()
        .next()
}

/// The text of `el` with runs of whitespace collapsed, leaving out [`NEVER_TEXT`] elements
/// and elements for which `skip` is true.
pub fn text_skipping(el: ElementRef, skip: &dyn Fn(ElementRef) -> bool) -> String {
    fn walk(node: NodeRef<Node>, skip: &dyn Fn(ElementRef) -> bool, out: &mut String) {
        for c in node.children() {
            match c.value() {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => {
                    let child = ElementRef::wrap(c).expect("element node");
                    if e.name() == "br" {
                        out.push(' ');
                    } else if !has_any_class(child, NEVER_TEXT) && !skip(child) {
                        walk(c, skip, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut raw = String::new();
    walk(*el, skip, &mut raw);
    collapse(&raw)
}

/// The text of `el` with runs of whitespace collapsed.
pub fn text(el: ElementRef) -> String {
    text_skipping(el, &|_| false)
}

pub fn collapse(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Some(text)` when not empty.
pub fn non_empty(text: String) -> Option<String> {
    Some(text).filter(|t| !t.is_empty())
}

/// The `sound://` file names of the links inside `el` for British and American
/// pronunciation (the links that contain a `t-audio-gb` / `t-audio-us` icon).
pub fn sounds(
    el: ElementRef,
    barrier: &dyn Fn(ElementRef) -> bool,
) -> (Option<String>, Option<String>) {
    let mut gb = None;
    let mut us = None;
    let links = scoped(
        el,
        &|e| {
            e.value().name() == "a"
                && e.value()
                    .attr("href")
                    .is_some_and(|h| h.starts_with("sound://"))
        },
        barrier,
    );
    for a in links {
        let name = a.value().attr("href").unwrap_or_default()["sound://".len()..].to_owned();
        if gb.is_none() && find(a, "t-audio-gb", &|_| false).is_some() {
            gb = Some(name);
        } else if us.is_none() && find(a, "t-audio-us", &|_| false).is_some() {
            us = Some(name);
        }
    }
    (gb, us)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn first<'a>(doc: &'a Html, class: &str) -> ElementRef<'a> {
        doc.root_element()
            .descendants()
            .filter_map(ElementRef::wrap)
            .find(|e| has_class(*e, class))
            .unwrap()
    }

    #[test]
    fn text_collapses_whitespace_and_skips_markers() {
        let doc = Html::parse_fragment(
            r#"<span class="a"> one <span class="t-ftindex">x</span><b>two</b>
               <span class="t-xsymb">◆</span><br>three <span class="skip">no</span></span>"#,
        );
        let a = first(&doc, "a");
        assert_eq!(text(a), "one two three no");
        assert_eq!(text_skipping(a, &|e| has_class(e, "skip")), "one two three");
    }

    #[test]
    fn scoped_search_stops_at_barriers_and_matches() {
        let doc = Html::parse_fragment(
            r#"<div class="root"><span class="m">1<span class="m">nested</span></span>
               <div class="wall"><span class="m">2</span></div><p><span class="m">3</span></p></div>"#,
        );
        let found = scoped(first(&doc, "root"), &|e| has_class(e, "m"), &|e| {
            has_class(e, "wall")
        });
        let texts: Vec<String> = found.into_iter().map(text).collect();
        assert_eq!(texts, ["1nested", "3"]);
    }

    #[test]
    fn sound_links() {
        let doc = Html::parse_fragment(
            r#"<div class="p"><a href="sound://audio/x__gb_1.ogg"><span class="t-audio-gb">🔊</span></a>
               <a href="sound://audio/x__us_1.ogg"><span class="t-audio-us">🔊</span></a></div>"#,
        );
        let (gb, us) = sounds(first(&doc, "p"), &|_| false);
        assert_eq!(gb.as_deref(), Some("audio/x__gb_1.ogg"));
        assert_eq!(us.as_deref(), Some("audio/x__us_1.ogg"));
    }
}
