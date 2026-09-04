//! NomadNet pages and a small Micron renderer.

/// A link found on a page, with the label shown to the user and the target
/// address/path to navigate to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Text displayed for the link.
    pub label: String,
    /// The `rns://<address>/<path>` target of the link.
    pub target: String,
}

/// Visual style of a rendered page line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStyle {
    /// Plain body text.
    Normal,
    /// A section heading (level 1–4).
    Heading(u8),
    /// Emphasised text (bold/italic/underline).
    Emphasized,
}

/// A single displayable line of a rendered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLine {
    pub text: String,
    pub style: PageStyle,
    /// Any links embedded in this line, in display order.
    pub links: Vec<Link>,
}

/// A fetched NomadNet page.
#[derive(Debug, Clone)]
pub struct Page {
    /// Human readable source (node address) of this page.
    pub source: String,
    /// Page title (first heading), if any.
    pub title: String,
    /// Raw page bytes (Micron markup).
    pub raw: Vec<u8>,
    lines: Vec<PageLine>,
}

impl Page {
    /// Parse raw Micron page bytes. `source` is the node address hash (hex)
    /// the page came from; it is used to resolve relative links.
    pub fn from_bytes(raw: &[u8], source: impl Into<String>) -> Page {
        let source = source.into();
        let text = String::from_utf8_lossy(raw);
        let (title, lines) = render_micron(text.as_ref(), &source);
        Page {
            source,
            title,
            raw: raw.to_vec(),
            lines,
        }
    }

    /// The rendered, displayable lines of the page.
    pub fn lines(&self) -> &[PageLine] {
        &self.lines
    }
}

/// Indentation (in spaces) per section depth level.
const SECTION_INDENT: usize = 2;
/// Maximum heading level we style distinctly.
const MAX_HEADING: u8 = 4;

/// Render Micron markup into plain display lines.
///
/// Implements the line-level Micron syntax (`#` comments, `\` escapes,
/// `>` headings, `<` depth reset, `` `= `` literal mode, `` `t `` tables,
/// `` `{...} `` partials) and the backtick inline formatting (`!` bold,
/// `*` italic, `_` underline, `` `F `` colours, `[label`target]` links,
/// `<field>` inputs). Colours and alignment are stripped; emphasis is
/// collapsed to [`PageStyle::Emphasized`].
pub fn render_micron(raw: &str, source: &str) -> (String, Vec<PageLine>) {
    let mut title = String::new();
    let mut lines = Vec::new();
    let mut literal = false;
    let mut depth = 0usize;
    let mut table_mode = false;
    let mut table_rows: Vec<String> = Vec::new();

    for raw_line in raw.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();

        // Toggle literal mode.
        if trimmed == "`=" {
            literal = !literal;
            continue;
        }

        // Toggle table mode.
        if trimmed.starts_with("`t") {
            if table_mode {
                if !table_rows.is_empty() {
                    render_table(&table_rows, &mut lines);
                    table_rows.clear();
                }
                table_mode = false;
            } else {
                table_mode = true;
                table_rows.clear();
            }
            continue;
        }

        if table_mode {
            table_rows.push(line.to_string());
            continue;
        }

        if literal {
            if !trimmed.is_empty() {
                push_line(&mut lines, trimmed.to_string(), PageStyle::Normal, Vec::new());
            }
            continue;
        }

        // Comment.
        if line.starts_with('#') {
            continue;
        }

        // Escaped line: display the rest literally.
        if let Some(rest) = line.strip_prefix('\\') {
            push_line(&mut lines, rest.to_string(), PageStyle::Normal, Vec::new());
            continue;
        }

        // Partial: show a placeholder so the reader knows there is content.
        if line.starts_with("`{") {
            let target = line[2..].split('`').next().unwrap_or("").trim().to_string();
            let text = if target.is_empty() {
                "[partial]".to_string()
            } else {
                format!("[partial: {target}]")
            };
            push_line(&mut lines, text, PageStyle::Emphasized, Vec::new());
            continue;
        }

        // Depth reset: back to top-level indentation.
        if line == "<" {
            depth = 0;
            continue;
        }

        // Heading (and section depth) from leading '>'.
        let leading = line.chars().take_while(|&c| c == '>').count();
        if leading > 0 {
            let text = line[leading..].trim();
            depth = leading;
            // Lines containing an inline field are not headings (matching the
            // reference parser, which strips the heading markers).
            if text.contains("`<") {
                let (text, style, links) = inline(text, source);
                push_line(&mut lines, indent(text, depth), style, links);
            } else {
                if title.is_empty() {
                    title = text.to_string();
                }
                let (text, _style, links) = inline(text, source);
                push_line(
                    &mut lines,
                    text,
                    PageStyle::Heading(leading.min(MAX_HEADING as usize) as u8),
                    links,
                );
            }
            continue;
        }

        // Plain paragraph, with inline formatting stripped and links extracted.
        let (text, style, links) = inline(line, source);
        push_line(&mut lines, indent(text, depth), style, links);
    }

    if table_mode {
        render_table(&table_rows, &mut lines);
    }

    (title, lines)
}

/// Prefix `text` with section indentation.
fn indent(text: String, depth: usize) -> String {
    let n = depth.saturating_sub(1).min(6) * SECTION_INDENT;
    let mut out = " ".repeat(n);
    out.push_str(text.trim_start());
    out
}

fn push_line(
    lines: &mut Vec<PageLine>,
    text: String,
    style: PageStyle,
    links: Vec<Link>,
) {
    if text.trim().is_empty() {
        return;
    }
    lines.push(PageLine { text, style, links });
}

/// Render a buffered table (rows of `|`-separated cells) as text lines.
fn render_table(rows: &[String], lines: &mut Vec<PageLine>) {
    for (idx, row) in rows.iter().enumerate() {
        if idx == 1 && is_alignment_row(row) {
            // Alignment row (e.g. `|:---|---:|`); nothing to display.
            continue;
        }
        let cells = split_cells(row);
        if cells.is_empty() {
            continue;
        }
        let text = cells.join(" | ");
        if text.trim().is_empty() {
            continue;
        }
        let style = if idx == 0 {
            PageStyle::Emphasized
        } else {
            PageStyle::Normal
        };
        push_line(lines, text, style, Vec::new());
    }
}

/// Split a table row into cells, honouring `\` escapes and stripping a
/// leading/trailing `|`.
fn split_cells(row: &str) -> Vec<String> {
    let row = row.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in row.chars() {
        if escaped {
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '|' {
            cells.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    cells.push(current.trim().to_string());
    cells
}

/// Whether a row is a table alignment marker row (`|---|:---:|`).
fn is_alignment_row(row: &str) -> bool {
    split_cells(row).iter().all(|cell| {
        let c = cell.trim().trim_start_matches(':').trim_end_matches(':');
        !c.is_empty() && c.chars().all(|ch| ch == '-')
    })
}

/// Modes of the inline (per-line) formatter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Formatting,
}

/// Parse inline formatting on a line, returning the visible text, whether any
/// emphasis was applied, and the links found.
fn inline(raw: &str, source: &str) -> (String, PageStyle, Vec<Link>) {
    let mut text = String::with_capacity(raw.len());
    let mut links = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut mode = Mode::Text;
    let mut escape = false;
    let mut emphasized = false;

    while i < chars.len() {
        let c = chars[i];
        match mode {
            Mode::Formatting => {
                match c {
                    '_' | '!' | '*' => {
                        // Toggle underline / bold / italic.
                        emphasized = true;
                        i += 1;
                    }
                    '`' => {
                        // End the formatting block.
                        mode = Mode::Text;
                        i += 1;
                    }
                    'F' => {
                        // Foreground colour: Fxxx, Fxxxxxx or FTxxxxxx.
                        i = skip_color(&chars, i);
                    }
                    'B' => {
                        // Background colour.
                        i = skip_color(&chars, i);
                    }
                    'f' | 'b' | 'c' | 'l' | 'r' | 'a' => {
                        // Reset / alignment directives.
                        i += 1;
                    }
                    ':' => {
                        // Anchor `:name — not displayable.
                        i += 1;
                        while i < chars.len()
                            && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-')
                        {
                            i += 1;
                        }
                    }
                    '<' => {
                        // Inline field: render its value.
                        i = parse_field(&chars, i, &mut text);
                    }
                    '[' => {
                        // Link.
                        i = parse_link(&chars, i, &mut text, &mut links, source);
                    }
                    _ => {
                        // Unknown formatting character: ignore.
                        i += 1;
                    }
                }
                // Each formatting directive stands alone; return to text mode.
                mode = Mode::Text;
            }
            Mode::Text => {
                if escape {
                    text.push(c);
                    escape = false;
                    i += 1;
                } else if c == '\\' {
                    escape = true;
                    i += 1;
                } else if c == '`' {
                    if i + 1 < chars.len() && chars[i + 1] == '`' {
                        // Double backtick: reset all formatting, stay in text mode.
                        i += 2;
                    } else {
                        // Enter formatting mode.
                        mode = Mode::Formatting;
                        i += 1;
                    }
                } else if c == '[' {
                    // Links are also recognised outside backticks for leniency.
                    i = parse_link(&chars, i, &mut text, &mut links, source);
                } else {
                    text.push(c);
                    i += 1;
                }
            }
        }
    }

    let style = if emphasized {
        PageStyle::Emphasized
    } else {
        PageStyle::Normal
    };
    (text, style, links)
}

/// Skip a Micron colour directive (`Fxxx`, `Fxxxxxx`, `FTxxxxxx`, `B...`).
/// `i` points at the directive letter.
fn skip_color(chars: &[char], i: usize) -> usize {
    if i + 1 < chars.len() && chars[i + 1] == 'T' {
        // FTxxxxxx / BTxxxxxx — 2 + 6 chars.
        i + 8
    } else if i + 4 <= chars.len() {
        // Fxxx / Bxxx — 1 + 3 chars.
        i + 4
    } else {
        i + 1
    }
}

/// Parse an inline field `<name`value>` (or `<flags|name`value>`), appending
/// its displayed value to `text`. Returns the new index (one past `>`).
fn parse_field(chars: &[char], i: usize, text: &mut String) -> usize {
    // Find the separating backtick.
    let mut backtick = i + 1;
    while backtick < chars.len() && chars[backtick] != '`' {
        backtick += 1;
    }
    // Find the closing '>'.
    let mut close = backtick + 1;
    while close < chars.len() && chars[close] != '>' {
        close += 1;
    }
    if backtick >= chars.len() || close >= chars.len() {
        // Not a valid field; show nothing (the reference drops invalid fields).
        return chars.len();
    }
    for c in &chars[backtick + 1..close] {
        text.push(*c);
    }
    close + 1
}

/// Parse a link `[label`target`fields]`, `[target]` or `[label](target)`,
/// appending the label to `text` and recording the link. Returns the new
/// index (one past the closing bracket). If the text is not a link, `[` is
/// appended literally.
fn parse_link(
    chars: &[char],
    i: usize,
    text: &mut String,
    links: &mut Vec<Link>,
    source: &str,
) -> usize {
    // Find the closing ']'.
    let mut end = i + 1;
    while end < chars.len() && chars[end] != ']' {
        end += 1;
    }
    if end >= chars.len() {
        text.push('[');
        return i + 1;
    }
    let inner: String = chars[i + 1..end].iter().collect();

    let (mut label, mut target, end_index) = if end + 1 < chars.len() && chars[end + 1] == '(' {
        // Markdown-style [label](target).
        let mut close = end + 2;
        while close < chars.len() && chars[close] != ')' {
            close += 1;
        }
        if close < chars.len() {
            let t: String = chars[end + 2..close].iter().collect();
            (inner.clone(), t, close + 1)
        } else {
            (inner.clone(), String::new(), end + 1)
        }
    } else {
        // Micron-style [label`target`fields] or [target].
        let parts: Vec<&str> = inner.split('`').collect();
        match parts.len() {
            1 => (inner.clone(), inner.clone(), end + 1),
            _ => (parts[0].to_string(), parts[1..].join("`"), end + 1),
        }
    };

    if target.trim().is_empty() {
        // A bracket with no target is just text.
        text.push_str(&inner);
        return end + 1;
    }

    // Resolve relative targets against the source node.
    target = normalize_target(&target, source);

    if label.trim().is_empty() {
        label = target.clone();
    }
    text.push_str(label.trim());
    links.push(Link {
        label: label.trim().to_string(),
        target,
    });
    end + 1
}

/// Normalise a link target to a navigable `rns://` URL where possible.
fn normalize_target(target: &str, source: &str) -> String {
    if let Some(rest) = target.strip_prefix("nomadnetwork://") {
        return format!("rns://{rest}");
    }
    if target.starts_with("rns://") || target.starts_with("#") || target.contains("://") {
        return target.to_string();
    }
    // Relative path (with or without a leading slash) — resolve against the
    // source node address.
    let path = target.trim_start_matches('/');
    format!("rns://{source}/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_comments_and_plain_text() {
        let micron = "\
>Main Title

# this is a comment

Plain paragraph with some text.

>>Sub Heading

Second paragraph.
";
        let (title, lines) = render_micron(micron, "aabb");
        assert_eq!(title, "Main Title");
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].style, PageStyle::Heading(1));
        assert_eq!(lines[0].text, "Main Title");
        assert_eq!(lines[1].text, "Plain paragraph with some text.");
        assert_eq!(lines[2].style, PageStyle::Heading(2));
        assert_eq!(lines[2].text, "Sub Heading");
        // Content under a sub-section is indented by one level.
        assert_eq!(lines[3].text, "  Second paragraph.");
    }

    #[test]
    fn backtick_emphasis() {
        let micron = "Text with `!bold`! and `*italics`* and `_underline`_ bits.\n";
        let (_title, lines) = render_micron(micron, "aabb");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Text with bold and italics and underline bits.");
        assert_eq!(lines[0].style, PageStyle::Emphasized);
    }

    #[test]
    fn extracts_micron_links() {
        let micron = "See `[home`rns://0123456789abcdef/page/index.mu] for more.\n";
        let (_title, lines) = render_micron(micron, "aabb");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "See home for more.");
        assert_eq!(lines[0].links.len(), 1);
        assert_eq!(lines[0].links[0].label, "home");
        assert_eq!(
            lines[0].links[0].target,
            "rns://0123456789abcdef/page/index.mu"
        );
    }

    #[test]
    fn resolves_relative_links_against_source() {
        let (_title, lines) = render_micron("`[Next`/page2.mu]", "0123456789abcdef");
        assert_eq!(
            lines[0].links[0].target,
            "rns://0123456789abcdef/page2.mu"
        );
        let (_title, lines) = render_micron("`[Next`page2.mu]", "0123456789abcdef");
        assert_eq!(
            lines[0].links[0].target,
            "rns://0123456789abcdef/page2.mu"
        );
    }

    #[test]
    fn comment_and_escape() {
        let (_title, lines) = render_micron("# comment\n\\# not a comment\n", "aabb");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "# not a comment");
    }

    #[test]
    fn literal_mode_and_tables() {
        let micron = "`=\n>Not a heading\n`=\n`t\n|Name|Value|\n|---|---|\n|A|1|\n`t\n";
        let (_title, lines) = render_micron(micron, "aabb");
        // Literal line + header row + one data row (alignment row skipped).
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.style != PageStyle::Heading(1)));
        assert_eq!(lines[0].text, ">Not a heading");
        assert_eq!(lines[0].style, PageStyle::Normal);
        assert_eq!(lines[1].style, PageStyle::Emphasized);
        assert_eq!(lines[1].text, "Name | Value");
        assert_eq!(lines[2].text, "A | 1");
    }

    #[test]
    fn sections_indent_content() {
        let micron = ">Top\n\nSome text.\n\n>>Sub\n\nNested text.\n";
        let (_title, lines) = render_micron(micron, "aabb");
        assert_eq!(lines[0].text, "Top");
        assert_eq!(lines[1].text, "Some text.");
        assert_eq!(lines[2].text, "Sub");
        assert!(lines[3].text.starts_with("  Nested text."));
    }

    #[test]
    fn renders_a_realistic_page() {
        let micron = "\
>Reticula News

# site announcement

Welcome to `!Reticula`! - the `_embedded`_ mesh client.

>>Latest

`t
|Version|Date|
|-------|-----|
|0.1.0  |2026-09-04|
`t

See `[the guide`/guide.mu] or `[home`rns://0123456789abcdef/index.mu].
";
        let (title, lines) = render_micron(micron, "0123456789abcdef");
        assert_eq!(title, "Reticula News");
        assert_eq!(lines[0].text, "Reticula News");
        assert!(lines[1].text.contains("Welcome to Reticula - the embedded mesh client."));
        assert!(lines[1].style == PageStyle::Emphasized);
        assert_eq!(lines[2].text, "Latest");
        assert_eq!(lines[3].text, "Version | Date");
        assert_eq!(lines[4].text, "0.1.0 | 2026-09-04");
        // The two links on the last line.
        let last = lines.last().unwrap();
        assert_eq!(last.links.len(), 2);
        assert_eq!(last.links[0].label, "the guide");
        assert_eq!(last.links[0].target, "rns://0123456789abcdef/guide.mu");
        assert_eq!(last.links[1].label, "home");
        assert_eq!(
            last.links[1].target,
            "rns://0123456789abcdef/index.mu"
        );
    }
}