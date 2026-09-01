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
    /// A section heading (level 1–3).
    Heading(u8),
    /// Emphasised text (bold/italic).
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
    /// Parse raw Micron page bytes.
    pub fn from_bytes(raw: &[u8], source: impl Into<String>) -> Page {
        let text = String::from_utf8_lossy(raw);
        let (title, lines) = render_micron(text.as_ref());
        Page {
            source: source.into(),
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

/// Render Micron markup into plain display lines.
///
/// Implements the subset of the Micron format relevant to handheld browsing:
/// `#` comments, `>` headings, `\` escapes, `*emphasis*`, `[label](rns://…)`
/// links, and plain paragraphs. Tables and complex fields are ignored.
pub fn render_micron(raw: &str) -> (String, Vec<PageLine>) {
    let mut title = String::new();
    let mut lines = Vec::new();

    for raw_line in raw.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('#') || line.starts_with("`=") {
            continue;
        }

        // Escaped line: display literally.
        if let Some(rest) = line.strip_prefix('\\') {
            lines.push(PageLine {
                text: rest.to_string(),
                style: PageStyle::Normal,
                links: Vec::new(),
            });
            continue;
        }

        // Heading: one or more leading '>'.
        let leading = line.chars().take_while(|&c| c == '>').count();
        if leading > 0 {
            let text = line[leading..].trim().to_string();
            if title.is_empty() {
                title = text.clone();
            }
            lines.push(PageLine {
                text,
                style: PageStyle::Heading(leading.min(3) as u8),
                links: Vec::new(),
            });
            continue;
        }

        // Plain paragraph, with inline formatting stripped and links extracted.
        let (text, links) = inline(line);
        let style = PageStyle::Normal;
        lines.push(PageLine { text, style, links });
    }

    (title, lines)
}

/// Parse inline formatting: `*bold*`, `_italic_` and `[label](rns://…)` links.
fn inline(raw: &str) -> (String, Vec<Link>) {
    let mut text = String::with_capacity(raw.len());
    let mut links = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Emphasis markers.
        if c == '*' || c == '_' {
            // Find a closing marker of the same char.
            if let Some(pos) = raw[i + 1..].find(c) {
                let inner: String = chars[i + 1..i + 1 + pos].iter().collect();
                text.push_str(&inner);
                i += pos + 2;
                continue;
            }
        }

        // Link: [label](target)
        if c == '[' {
            if let Some(close) = raw[i..].find("](") {
                let label: String = chars[i + 1..i + close].iter().collect();
                let rest = &raw[i + close + 2..];
                if let Some(end) = rest.find(')') {
                    let target = &rest[..end];
                    if target.starts_with("rns://") {
                        text.push_str(&label);
                        links.push(Link {
                            label: label.trim().to_string(),
                            target: target.to_string(),
                        });
                        i += close + 2 + end + 1;
                        continue;
                    }
                }
            }
        }

        text.push(c);
        i += 1;
    }

    (text, links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_comments_and_plain_text() {
        let micron = "\
>Main Title

# this is a comment

Plain paragraph with *bold* and _italic_ bits.

>>Sub Heading

Second paragraph.
";
        let (title, lines) = render_micron(micron);
        assert_eq!(title, "Main Title");
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].style, PageStyle::Heading(1));
        assert_eq!(lines[0].text, "Main Title");
        assert_eq!(lines[1].text, "Plain paragraph with bold and italic bits.");
        assert_eq!(lines[2].style, PageStyle::Heading(2));
        assert_eq!(lines[2].text, "Sub Heading");
    }

    #[test]
    fn extracts_links() {
        let micron = "See [home](rns://0123456789abcdef/page/index.mu) for more.";
        let (_title, lines) = render_micron(micron);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "See home for more.");
        assert_eq!(lines[0].links.len(), 1);
        assert_eq!(lines[0].links[0].label, "home");
        assert_eq!(lines[0].links[0].target, "rns://0123456789abcdef/page/index.mu");
    }

    #[test]
    fn comment_and_escape() {
        let (_title, lines) = render_micron("# comment\n\\# not a comment\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "# not a comment");
    }
}