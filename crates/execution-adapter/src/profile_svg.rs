//! Flame-graph renderer for [`FoldedProfile`] (ADR-074 §5, ADR-076 §5/§7).
//!
//! The product renders the SVG itself precisely so that sanitization is a
//! property of construction and not an inspection of somebody else's bytes
//! (ADR-074 §1). The document is static: only `<svg>`, `<style>`, `<g>`,
//! `<title>`, `<rect>` and `<text>` are ever emitted, with a fixed attribute
//! set (`xmlns`, `width`, `height`, `viewBox`, `class`, `x`, `y`). There is no
//! script, no event attribute, no link, no external reference and no URL other
//! than the one mandatory SVG namespace literal.
//!
//! Everything that originates outside this module — frame names from the
//! guest helper, the caller's title — passes through [`escape`], which is
//! deliberately stronger than XML escaping: besides `&<>"'` it replaces `:`
//! and `=` with numeric character references and breaks the one forbidden
//! token that is spelled with letters alone (`href`). That makes the byte
//! contract hold by construction: no `javascript:`, `data:`, `xlink:`,
//! `http://` or `on…=` sequence can be assembled out of untrusted text, even
//! though `:` is legal inside a sanitized Rust symbol name.
//!
//! Two more invariants:
//!
//! * A profile with no stacks is a valid document carrying a "no samples"
//!   label. ADR-076 §5 declares zero samples a result, not a failure.
//! * The 8 MiB artifact ceiling of ADR-076 §7 is enforced by re-rendering at a
//!   smaller depth, never by truncating the byte stream mid-element.
// Same rationale as `profile_stacks`: the consuming gateway is a separate
// deliverable, so the crate has no non-test caller yet.
#![allow(dead_code)]

use crate::profile_stacks::FoldedProfile;
use std::collections::BTreeMap;
use std::fmt;

/// Artifact ceiling of ADR-076 §7. `SvgOptions::max_bytes` may lower it, never
/// raise it.
pub(crate) const MAX_SVG_BYTES: usize = 8 * 1024 * 1024;
/// Hard cap on rendered levels, independent of the caller's `max_depth`. It
/// also bounds the depth of the merged tree, hence the recursion here.
pub(crate) const MAX_RENDER_DEPTH: usize = 512;
/// Printable-ASCII title ceiling.
pub(crate) const MAX_TITLE_BYTES: usize = 120;
/// Frames narrower than this fraction of the canvas are omitted and counted
/// instead of being emitted as invisible elements.
const MIN_VISIBLE_FRACTION: f64 = 0.001;
const MIN_WIDTH: u32 = 200;
const MAX_WIDTH: u32 = 20_000;
const MIN_FRAME_HEIGHT: u32 = 8;
const MAX_FRAME_HEIGHT: u32 = 64;
/// Rows reserved above the flame graph (title, subtitle, separator) plus one
/// below it.
const HEADER_ROWS: u64 = 3;
const FOOTER_ROWS: u64 = 1;
/// Approximate advance of the 11px monospace face, used only to decide how
/// much of a label fits. It never affects geometry.
const LABEL_ADVANCE: f64 = 6.6;
/// Fixed warm palette. Selection is a hash of the frame name, so the same
/// symbol always gets the same colour and nothing is random.
const PALETTE: [&str; 8] = [
    "#f5c26b", "#f0a35e", "#eb8a54", "#e4704a", "#d95b3f", "#c94a35", "#e9b17a", "#d8853c",
];

/// Rendering knobs. The defaults are the ADR-076 §7 artifact budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SvgOptions {
    pub width: u32,
    pub frame_height: u32,
    pub max_depth: usize,
    pub max_bytes: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            width: 1200,
            frame_height: 16,
            max_depth: 64,
            max_bytes: MAX_SVG_BYTES,
        }
    }
}

/// Closed failure vocabulary. No variant carries guest-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SvgError {
    /// The profile declares stacks but yields no renderable weight (no frames,
    /// or a zero total), so there is no denominator to scale widths by. A
    /// profile with *no stacks at all* is not this error: it renders the
    /// "no samples" document.
    Empty,
    /// The document does not fit in the byte budget even with every level
    /// dropped.
    TooLarge,
    /// The title is longer than [`MAX_TITLE_BYTES`] or is not printable ASCII.
    InvalidTitle,
    /// Options that cannot produce a document (canvas outside the supported
    /// range) or geometry that would overflow the SVG coordinate space.
    Internal,
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("flamegraph could not be rendered")
    }
}
impl std::error::Error for SvgError {}

/// One node of the merged call tree. Children live in a `BTreeMap`, so sibling
/// order is the frame name ascending and the document is deterministic.
#[derive(Debug, Default)]
struct Node {
    total: u64,
    children: BTreeMap<String, Node>,
}

fn build_tree(profile: &FoldedProfile, depth_limit: usize) -> BTreeMap<String, Node> {
    let mut roots: BTreeMap<String, Node> = BTreeMap::new();
    for stack in &profile.stacks {
        let mut level = &mut roots;
        for frame in stack.frames.iter().take(depth_limit) {
            let node = level.entry(frame.clone()).or_default();
            node.total = node.total.saturating_add(stack.count);
            level = &mut node.children;
        }
    }
    roots
}

/// Depth of the merged tree. Bounded by the `depth_limit` used to build it.
fn tree_depth(level: &BTreeMap<String, Node>) -> usize {
    level
        .values()
        .map(|node| 1usize.saturating_add(tree_depth(&node.children)))
        .max()
        .unwrap_or(0)
}

/// Node count of a subtree, used to declare how many frames were omitted.
fn subtree_size(node: &Node) -> u64 {
    node.children.values().fold(1u64, |total, child| {
        total.saturating_add(subtree_size(child))
    })
}

/// Deterministic FNV-1a over the frame name, folded into [`PALETTE`].
fn palette_index(name: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % PALETTE.len() as u64) as usize
}

/// True when `chars[index..]` starts with `href`, ignoring ASCII case.
fn starts_href(chars: &[char], index: usize) -> bool {
    const TOKEN: [char; 4] = ['h', 'r', 'e', 'f'];
    chars
        .get(index..index.saturating_add(TOKEN.len()))
        .is_some_and(|window| {
            window
                .iter()
                .zip(TOKEN.iter())
                .all(|(candidate, expected)| candidate.eq_ignore_ascii_case(expected))
        })
}

/// Escapes one text run so that no forbidden construct can survive it.
///
/// `&<>"'` become the predefined XML entities. `:` and `=` become numeric
/// character references, which renders identically but makes `javascript:`,
/// `data:`, `xlink:`, `http://` and any `on…=` attribute shape unrepresentable
/// in the output — a sanitized frame name may legitimately contain `:`. `href`
/// is the single forbidden token spelled with letters only, so its leading `h`
/// is emitted as `&#104;`. Anything outside printable ASCII becomes `_`;
/// control characters have no valid character reference in XML 1.0 and the
/// upstream alphabets never contain one.
fn escape(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < chars.len() {
        if starts_href(&chars, index) {
            out.push_str("&#104;");
            index = index.saturating_add(1);
            continue;
        }
        match chars.get(index).copied() {
            Some('&') => out.push_str("&amp;"),
            Some('<') => out.push_str("&lt;"),
            Some('>') => out.push_str("&gt;"),
            Some('"') => out.push_str("&quot;"),
            Some('\'') => out.push_str("&apos;"),
            Some(':') => out.push_str("&#58;"),
            Some('=') => out.push_str("&#61;"),
            Some(other) if other.is_ascii_graphic() || other == ' ' => out.push(other),
            _ => out.push('_'),
        }
        index = index.saturating_add(1);
    }
    out
}

fn validate_title(title: &str) -> Result<(), SvgError> {
    if title.len() > MAX_TITLE_BYTES {
        return Err(SvgError::InvalidTitle);
    }
    if !title.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(SvgError::InvalidTitle);
    }
    Ok(())
}

/// Longest label that fits inside a rectangle, or `None` when not even three
/// characters fit. Truncation is marked with `..`.
fn label(name: &str, width: f64) -> Option<String> {
    let capacity = ((width - 4.0) / LABEL_ADVANCE) as i64;
    let capacity = usize::try_from(capacity).ok()?;
    if capacity < 3 {
        return None;
    }
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= capacity {
        return Some(name.to_owned());
    }
    let mut clipped: String = chars.get(..capacity.saturating_sub(2))?.iter().collect();
    clipped.push_str("..");
    Some(clipped)
}

fn style_block() -> String {
    let mut css = String::from(
        "<style>\n\
         text{font-family:monospace;font-size:11px;fill:#231a12}\n\
         text.hdr{font-size:15px;font-weight:bold}\n\
         text.sub{fill:#6d5b48}\n\
         text.note{font-size:13px;fill:#6d5b48}\n\
         rect{stroke:#fdf8f1;stroke-width:0.5}\n\
         rect.bg{fill:#fdf8f1;stroke:none}\n",
    );
    for (index, colour) in PALETTE.iter().enumerate() {
        css.push_str(&format!("rect.c{index}{{fill:{colour}}}\n"));
    }
    css.push_str("</style>\n");
    css
}

/// Everything one render pass needs that does not change between passes.
struct Plan<'a> {
    roots: &'a BTreeMap<String, Node>,
    title: &'a str,
    options: &'a SvgOptions,
    budget: usize,
    denominator: u64,
    stacks: usize,
}

/// Mutable state of one pass: the frame body plus the omission counters that
/// the subtitle declares.
struct Canvas<'a> {
    plan: &'a Plan<'a>,
    cap: usize,
    top: f64,
    body: String,
    narrow: u64,
    deep: u64,
}

impl Canvas<'_> {
    fn scale(&self, samples: u64) -> f64 {
        samples as f64 / self.plan.denominator as f64 * f64::from(self.plan.options.width)
    }

    fn emit(
        &mut self,
        level: &BTreeMap<String, Node>,
        depth: usize,
        offset: u64,
    ) -> Result<(), SvgError> {
        let mut cursor = offset;
        for (name, node) in level {
            let start = cursor;
            cursor = cursor.saturating_add(node.total);
            if depth >= self.cap {
                self.deep = self.deep.saturating_add(subtree_size(node));
                continue;
            }
            let width = self.scale(node.total);
            if width < f64::from(self.plan.options.width) * MIN_VISIBLE_FRACTION {
                self.narrow = self.narrow.saturating_add(subtree_size(node));
                continue;
            }
            let height = f64::from(self.plan.options.frame_height);
            let x = self.scale(start);
            let y = self.top + depth as f64 * height;
            let percent = node.total as f64 * 100.0 / self.plan.denominator as f64;
            let escaped = escape(name);
            self.body.push_str(&format!(
                "<g><title>{escaped} {total} samples {percent:.2}%</title>\
                 <rect class=\"c{palette}\" x=\"{x:.3}\" y=\"{y:.3}\" width=\"{width:.3}\" height=\"{height:.3}\"/>",
                total = node.total,
                palette = palette_index(name),
            ));
            if let Some(text) = label(name, width) {
                self.body.push_str(&format!(
                    "<text x=\"{label_x:.3}\" y=\"{label_y:.3}\">{}</text>",
                    escape(&text),
                    label_x = x + 2.0,
                    label_y = y + height * 0.72,
                ));
            }
            self.body.push_str("</g>\n");
            // Abandon the whole pass rather than emit a partial element; the
            // caller retries at a smaller depth.
            if self.body.len() > self.plan.budget {
                return Err(SvgError::TooLarge);
            }
            self.emit(&node.children, depth.saturating_add(1), start)?;
        }
        Ok(())
    }
}

fn render_document(plan: &Plan<'_>, cap: usize) -> Result<String, SvgError> {
    let frame_height = u64::from(plan.options.frame_height);
    let levels = if plan.roots.is_empty() { 0 } else { cap };
    let rows = u64::try_from(levels)
        .ok()
        .and_then(|levels| levels.checked_add(HEADER_ROWS))
        .and_then(|rows| rows.checked_add(FOOTER_ROWS))
        .ok_or(SvgError::Internal)?;
    let height = frame_height
        .checked_mul(rows)
        .and_then(|height| u32::try_from(height).ok())
        .ok_or(SvgError::Internal)?;
    let width = plan.options.width;
    let row = f64::from(plan.options.frame_height);

    let mut canvas = Canvas {
        plan,
        cap,
        top: row * HEADER_ROWS as f64,
        body: String::new(),
        narrow: 0,
        deep: 0,
    };
    if plan.roots.is_empty() {
        canvas.body.push_str(&format!(
            "<text class=\"note\" x=\"8\" y=\"{y:.3}\">{}</text>\n",
            escape("no samples collected"),
            y = row * (HEADER_ROWS as f64 + 0.75),
        ));
    } else {
        canvas.emit(plan.roots, 0, 0)?;
    }

    let subtitle = format!(
        "samples {} | stacks {} | levels {} | omitted {} narrow | omitted {} beyond depth",
        plan.denominator, plan.stacks, levels, canvas.narrow, canvas.deep
    );
    let mut document = String::with_capacity(canvas.body.len().saturating_add(1024));
    document.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    ));
    document.push_str(&format!("<title>{}</title>\n", escape(plan.title)));
    document.push_str(&style_block());
    document.push_str(&format!(
        "<rect class=\"bg\" x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\"/>\n"
    ));
    document.push_str(&format!(
        "<text class=\"hdr\" x=\"8\" y=\"{y:.3}\">{}</text>\n",
        escape(plan.title),
        y = row * 0.95,
    ));
    document.push_str(&format!(
        "<text class=\"sub\" x=\"8\" y=\"{y:.3}\">{}</text>\n",
        escape(&subtitle),
        y = row * 1.95,
    ));
    document.push_str("<g class=\"frames\">\n");
    document.push_str(&canvas.body);
    document.push_str("</g>\n</svg>\n");
    if document.len() > plan.budget {
        return Err(SvgError::TooLarge);
    }
    Ok(document)
}

/// Renders `profile` as a sanitized, static flame graph.
///
/// Widths are proportional to inclusive sample counts, siblings are ordered by
/// frame name, and colours come from a hash of the name: rendering the same
/// profile twice yields identical bytes. If the document exceeds the byte
/// budget the deepest level is dropped and the whole document is rendered
/// again; only when nothing fits is [`SvgError::TooLarge`] returned.
pub(crate) fn render(
    profile: &FoldedProfile,
    title: &str,
    options: SvgOptions,
) -> Result<Vec<u8>, SvgError> {
    validate_title(title)?;
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&options.width)
        || !(MIN_FRAME_HEIGHT..=MAX_FRAME_HEIGHT).contains(&options.frame_height)
    {
        return Err(SvgError::Internal);
    }
    let depth_limit = options.max_depth.min(MAX_RENDER_DEPTH);
    // The tree is built to the hard cap, not to `max_depth`, so that levels
    // dropped by the caller's cap are counted as omitted instead of vanishing.
    let roots = build_tree(profile, MAX_RENDER_DEPTH);
    let denominator = roots
        .values()
        .fold(0u64, |total, node| total.saturating_add(node.total));
    if !profile.stacks.is_empty() && denominator == 0 {
        return Err(SvgError::Empty);
    }
    let plan = Plan {
        roots: &roots,
        title,
        options: &options,
        budget: options.max_bytes.min(MAX_SVG_BYTES),
        denominator,
        stacks: profile.stacks.len(),
    };
    let mut cap = depth_limit.min(tree_depth(&roots));
    loop {
        match render_document(&plan, cap) {
            Ok(document) => return Ok(document.into_bytes()),
            Err(SvgError::TooLarge) => {
                if cap == 0 {
                    return Err(SvgError::TooLarge);
                }
                cap = cap.saturating_sub(1);
            }
            Err(other) => return Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_stacks::{FoldedStack, parse_folded};

    /// Minimal structural check: every element is closed, in order, and the
    /// document ends on an element boundary.
    fn is_well_formed(document: &str) -> bool {
        let mut open: Vec<&str> = Vec::new();
        let mut rest = document;
        while let Some(start) = rest.find('<') {
            let tail = match rest.get(start.saturating_add(1)..) {
                Some(tail) => tail,
                None => return false,
            };
            let Some(end) = tail.find('>') else {
                return false;
            };
            let (tag, remainder) = match (tail.get(..end), tail.get(end.saturating_add(1)..)) {
                (Some(tag), Some(remainder)) => (tag, remainder),
                _ => return false,
            };
            if let Some(name) = tag.strip_prefix('/') {
                if open.pop() != Some(name) {
                    return false;
                }
            } else if !tag.ends_with('/') {
                open.push(tag.split([' ', '\n']).next().unwrap_or(tag));
            }
            rest = remainder;
        }
        open.is_empty() && document.ends_with("</svg>\n")
    }

    /// Every `name=` occurrence in the document, so a test can prove that no
    /// event attribute was emitted whatever the input contained.
    fn attribute_names(document: &str) -> Vec<&str> {
        let bytes = document.as_bytes();
        let mut names = Vec::new();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b'=' {
                continue;
            }
            let mut start = index;
            while start > 0
                && bytes.get(start.saturating_sub(1)).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || *byte == b':' || *byte == b'-'
                })
            {
                start = start.saturating_sub(1);
            }
            if let Some(name) = document.get(start..index) {
                names.push(name);
            }
        }
        names
    }

    fn document(bytes: &[u8]) -> Result<String, SvgError> {
        String::from_utf8(bytes.to_vec()).map_err(|_| SvgError::Internal)
    }

    fn sample_profile() -> Result<FoldedProfile, SvgError> {
        parse_folded(
            b"main;compute;hash 40\n\
              main;compute;mix 25\n\
              main;io::read 20\n\
              main;io::write 10\n\
              worker;park 5\n",
        )
        .map_err(|_| SvgError::Internal)
    }

    #[test]
    fn renders_a_profile_into_a_well_formed_static_document() -> Result<(), SvgError> {
        let rendered = render(&sample_profile()?, "profile sample", SvgOptions::default())?;
        let text = document(&rendered)?;
        assert!(text.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(is_well_formed(&text));
        assert!(text.contains("<title>profile sample</title>"));
        assert!(text.contains("samples 100 | stacks 5"));
        // Only the five allowed element names are ever opened.
        for element in ["svg", "style", "title", "g", "rect", "text"] {
            assert!(text.contains(&format!("<{element}")), "missing {element}");
        }
        Ok(())
    }

    #[test]
    fn zero_samples_parses_and_renders_a_valid_document_end_to_end() -> Result<(), SvgError> {
        let profile = parse_folded(b"").map_err(|_| SvgError::Internal)?;
        assert_eq!(profile.total_samples, 0);
        let rendered = render(&profile, "empty run", SvgOptions::default())?;
        let text = document(&rendered)?;
        assert!(is_well_formed(&text));
        assert!(text.contains("no samples collected"));
        assert!(text.contains("samples 0 | stacks 0"));
        assert!(!text.contains("<rect class=\"c"));
        Ok(())
    }

    #[test]
    fn a_profile_with_stacks_but_no_weight_is_the_empty_error() {
        let profile = FoldedProfile {
            stacks: vec![FoldedStack {
                frames: vec!["main".to_owned()],
                count: 0,
            }],
            ..FoldedProfile::default()
        };
        assert_eq!(
            render(&profile, "no weight", SvgOptions::default()),
            Err(SvgError::Empty)
        );
    }

    #[test]
    fn unsupported_canvas_options_are_internal_not_a_document() -> Result<(), SvgError> {
        let profile = sample_profile()?;
        for options in [
            SvgOptions {
                width: 10,
                ..SvgOptions::default()
            },
            SvgOptions {
                width: MAX_WIDTH + 1,
                ..SvgOptions::default()
            },
            SvgOptions {
                frame_height: 0,
                ..SvgOptions::default()
            },
            SvgOptions {
                frame_height: MAX_FRAME_HEIGHT + 1,
                ..SvgOptions::default()
            },
        ] {
            assert_eq!(
                render(&profile, "bad canvas", options),
                Err(SvgError::Internal)
            );
        }
        Ok(())
    }

    #[test]
    fn titles_are_bounded_and_printable_ascii() -> Result<(), SvgError> {
        let profile = sample_profile()?;
        for title in [
            "a".repeat(MAX_TITLE_BYTES + 1),
            "line\nbreak".to_owned(),
            "tab\there".to_owned(),
            "caf\u{e9}".to_owned(),
            "\u{7f}".to_owned(),
        ] {
            assert_eq!(
                render(&profile, &title, SvgOptions::default()),
                Err(SvgError::InvalidTitle)
            );
        }
        assert!(
            render(
                &profile,
                &"a".repeat(MAX_TITLE_BYTES),
                SvgOptions::default()
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn rendered_bytes_carry_no_active_construct_even_for_hostile_frames() -> Result<(), SvgError> {
        // Every one of these frame names is inside the sanitized alphabet, so
        // the helper could legitimately hand them over.
        let profile = parse_folded(
            b"<script>;javascript:alert 30\n\
              <script>;xlink:x 20\n\
              data:image;<a 15\n\
              data:image;<foreignObject> 20\n\
              data:image;<use> 15\n",
        )
        .map_err(|_| SvgError::Internal)?;
        let rendered = render(
            &profile,
            "hostile: <script> & 'x' http://evil.example onload=1",
            SvgOptions::default(),
        )?;
        let text = document(&rendered)?;
        assert!(is_well_formed(&text));
        for forbidden in [
            "<script",
            "href",
            "xlink:",
            "<foreignObject",
            "<image",
            "<use",
            "<a ",
            "<!ENTITY",
            "<!DOCTYPE",
            "<!--",
            "javascript:",
            "data:",
            "https://",
        ] {
            assert!(!text.contains(forbidden), "found {forbidden}");
        }
        // The single documented exception: the mandatory namespace literal.
        assert_eq!(text.matches("http://").count(), 1);
        assert!(text.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        // No `on…=` attribute, whatever the title or the frame names said.
        for name in attribute_names(&text) {
            assert!(!name.starts_with("on"), "event attribute {name}");
        }
        // The hostile text is still present, escaped and inert.
        assert!(text.contains("&lt;script&gt;"));
        assert!(text.contains("javascript&#58;alert"));
        Ok(())
    }

    #[test]
    fn xml_special_characters_are_escaped_in_text_and_titles() -> Result<(), SvgError> {
        let profile = parse_folded(b"<T>::f<&U> 5\n").map_err(|_| SvgError::Internal)?;
        let rendered = render(&profile, "quotes \" ' & <angles>", SvgOptions::default())?;
        let text = document(&rendered)?;
        assert!(is_well_formed(&text));
        assert!(text.contains("&lt;T&gt;&#58;&#58;f&lt;&amp;U&gt;"));
        assert!(text.contains("quotes &quot; &apos; &amp; &lt;angles&gt;"));
        assert_eq!(escape("&<>\"'"), "&amp;&lt;&gt;&quot;&apos;");
        assert_eq!(escape("xlink:href"), "xlink&#58;&#104;ref");
        assert_eq!(escape("onload=1"), "onload&#61;1");
        assert_eq!(escape("caf\u{e9}"), "caf_");
        Ok(())
    }

    #[test]
    fn rendering_the_same_profile_twice_yields_identical_bytes() -> Result<(), SvgError> {
        let profile = sample_profile()?;
        let first = render(&profile, "determinism", SvgOptions::default())?;
        let second = render(&profile, "determinism", SvgOptions::default())?;
        assert_eq!(first, second);
        // Colour selection is a pure function of the name.
        assert_eq!(palette_index("main"), palette_index("main"));
        Ok(())
    }

    #[test]
    fn frames_narrower_than_a_thousandth_are_omitted_and_declared() -> Result<(), SvgError> {
        let mut folded = String::from("main;wide 100000\n");
        folded.push_str("main;zzz_narrow;deeper 1\n");
        let profile = parse_folded(folded.as_bytes()).map_err(|_| SvgError::Internal)?;
        let rendered = render(&profile, "narrow", SvgOptions::default())?;
        let text = document(&rendered)?;
        // The narrow node and its child are both accounted for, and neither is
        // emitted as an invisible element.
        assert!(text.contains("omitted 2 narrow"));
        assert!(!text.contains("zzz_narrow"));
        assert!(text.contains("wide"));
        Ok(())
    }

    #[test]
    fn depth_beyond_the_cap_is_dropped_and_declared() -> Result<(), SvgError> {
        let profile = parse_folded(b"a;b;c;d 8\n").map_err(|_| SvgError::Internal)?;
        let rendered = render(
            &profile,
            "depth",
            SvgOptions {
                max_depth: 2,
                ..SvgOptions::default()
            },
        )?;
        let text = document(&rendered)?;
        assert!(text.contains("levels 2"));
        assert!(text.contains(">a<"));
        assert!(text.contains(">b<"));
        assert!(!text.contains(">c<"));
        // `c` and `d` are dropped by the cap, and declared rather than hidden.
        assert!(text.contains("omitted 2 beyond depth"));
        Ok(())
    }

    fn deep_profile() -> Result<FoldedProfile, SvgError> {
        let mut folded = String::new();
        for index in 0..40u32 {
            let mut stack = String::new();
            for level in 0..8u32 {
                if level > 0 {
                    stack.push(';');
                }
                stack.push_str(&format!("frame_{index:03}_{level}"));
            }
            folded.push_str(&format!("{stack} 5\n"));
        }
        parse_folded(folded.as_bytes()).map_err(|_| SvgError::Internal)
    }

    #[test]
    fn an_oversized_document_is_re_rendered_at_a_smaller_depth() -> Result<(), SvgError> {
        let profile = deep_profile()?;
        let full = render(&profile, "budget", SvgOptions::default())?;
        let budget = full.len() / 2;
        let shrunk = render(
            &profile,
            "budget",
            SvgOptions {
                max_bytes: budget,
                ..SvgOptions::default()
            },
        )?;
        assert!(shrunk.len() <= budget);
        assert!(shrunk.len() < full.len());
        let text = document(&shrunk)?;
        // Shrinking drops whole levels; it never truncates an element.
        assert!(is_well_formed(&text));
        assert!(text.matches("<rect").count() < document(&full)?.matches("<rect").count());
        Ok(())
    }

    #[test]
    fn a_budget_that_nothing_fits_is_too_large() -> Result<(), SvgError> {
        assert_eq!(
            render(
                &deep_profile()?,
                "tiny budget",
                SvgOptions {
                    max_bytes: 200,
                    ..SvgOptions::default()
                },
            ),
            Err(SvgError::TooLarge)
        );
        Ok(())
    }

    #[test]
    fn the_artifact_ceiling_cannot_be_raised_by_the_caller() -> Result<(), SvgError> {
        let profile = sample_profile()?;
        let raised = render(
            &profile,
            "ceiling",
            SvgOptions {
                max_bytes: usize::MAX,
                ..SvgOptions::default()
            },
        )?;
        assert!(raised.len() <= MAX_SVG_BYTES);
        Ok(())
    }

    /// Renders the sample the integrator eyeballs. The file is only written
    /// when `RUST_MCP_PROFILE_SVG_SAMPLE` names a path, so a normal test run
    /// touches no filesystem.
    #[test]
    fn renders_a_sample_document_for_manual_inspection() -> Result<(), SvgError> {
        let rendered = render(
            &sample_profile()?,
            "rust-mcp profiling sample",
            SvgOptions::default(),
        )?;
        assert!(is_well_formed(&document(&rendered)?));
        if let Ok(path) = std::env::var("RUST_MCP_PROFILE_SVG_SAMPLE") {
            let _ = std::fs::write(path, &rendered);
        }
        Ok(())
    }
}
