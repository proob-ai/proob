//! A small XML reader for the reports proob consumes (JUnit from nextest):
//! elements, attributes, text, CDATA, comments, processing instructions, and
//! the five predefined entities plus numeric character references. No
//! namespaces, no DTDs, no external entities: a document using them is
//! refused rather than guessed at.
use crate::error::ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Child elements with this name, in document order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter_map(move |n| match n {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// Every element with this name at any depth below, in document order.
    pub fn descendants_named<'a>(&'a self, name: &'a str) -> Vec<&'a Element> {
        let mut found = Vec::new();
        collect_named(self, name, &mut found);
        found
    }

    /// The concatenated text of this element's direct text children.
    pub fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }
}

fn collect_named<'a>(e: &'a Element, name: &str, found: &mut Vec<&'a Element>) {
    for child in &e.children {
        if let Node::Element(c) = child {
            if c.name == name {
                found.push(c);
            }
            collect_named(c, name, found);
        }
    }
}

/// Parses one document and returns its root element.
pub fn parse(text: &str) -> Result<Element, ParseError> {
    let mut p = Parser { s: text, at: 0 };
    p.skip_misc()?;
    let root = p.element()?;
    p.skip_misc()?;
    if p.at != p.s.len() {
        return Err(p.error("content after the root element"));
    }
    Ok(root)
}

struct Parser<'a> {
    s: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, what: &str) -> ParseError {
        ParseError::Schema(format!("xml: {what} at byte {}", self.at))
    }

    fn rest(&self) -> &'a str {
        &self.s[self.at..]
    }

    fn starts(&self, p: &str) -> bool {
        self.rest().starts_with(p)
    }

    fn eat(&mut self, p: &str) -> bool {
        let yes = self.starts(p);
        if yes {
            self.at += p.len();
        }
        yes
    }

    fn skip_ws(&mut self) {
        self.at += self.rest().len() - self.rest().trim_start().len();
    }

    /// Skips text up to and including `end`, or fails naming `what`.
    fn skip_through(&mut self, end: &str, what: &str) -> Result<(), ParseError> {
        match self.rest().find(end) {
            Some(i) => {
                self.at += i + end.len();
                Ok(())
            }
            None => Err(self.error(&format!("unterminated {what}"))),
        }
    }

    /// Whitespace, comments, processing instructions, and a doctype
    /// declaration (refused: proob reads no DTD).
    fn skip_misc(&mut self) -> Result<(), ParseError> {
        loop {
            self.skip_ws();
            if self.eat("<?") {
                self.skip_through("?>", "processing instruction")?;
            } else if self.eat("<!--") {
                self.skip_through("-->", "comment")?;
            } else if self.starts("<!DOCTYPE") {
                return Err(self.error("DOCTYPE is not supported"));
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<String, ParseError> {
        let n = self.rest().bytes().take_while(|b| name_byte(*b)).count();
        if n == 0 {
            return Err(self.error("expected a name"));
        }
        let name = self.rest()[..n].to_string();
        self.at += n;
        Ok(name)
    }

    fn element(&mut self) -> Result<Element, ParseError> {
        if !self.eat("<") {
            return Err(self.error("expected `<`"));
        }
        let name = self.name()?;
        let attrs = self.attrs()?;
        if self.eat("/>") {
            return Ok(Element { name, attrs, children: Vec::new() });
        }
        if !self.eat(">") {
            return Err(self.error("expected `>`"));
        }
        let children = self.children(&name)?;
        Ok(Element { name, attrs, children })
    }

    fn attrs(&mut self) -> Result<Vec<(String, String)>, ParseError> {
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            if self.starts("/>") || self.starts(">") {
                return Ok(attrs);
            }
            let key = self.name()?;
            self.skip_ws();
            if !self.eat("=") {
                return Err(self.error("expected `=` after an attribute name"));
            }
            self.skip_ws();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(self.error("expected a quoted attribute value")),
            };
            self.at += 1;
            let end = self.rest().find(quote).ok_or_else(|| self.error("unterminated attribute value"))?;
            let raw = &self.rest()[..end];
            let value = unescape(raw).map_err(|e| self.error(&e))?;
            self.at += end + 1;
            attrs.push((key, value));
        }
    }

    fn children(&mut self, parent: &str) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            if self.eat("</") {
                let name = self.name()?;
                self.skip_ws();
                if name != parent || !self.eat(">") {
                    return Err(self.error(&format!("mismatched end tag `</{name}>` for `<{parent}>`")));
                }
                return Ok(nodes);
            }
            if self.eat("<![CDATA[") {
                let end = self.rest().find("]]>").ok_or_else(|| self.error("unterminated CDATA"))?;
                nodes.push(Node::Text(self.rest()[..end].to_string()));
                self.at += end + 3;
            } else if self.eat("<!--") {
                self.skip_through("-->", "comment")?;
            } else if self.eat("<?") {
                self.skip_through("?>", "processing instruction")?;
            } else if self.starts("<") {
                nodes.push(Node::Element(self.element()?));
            } else if self.at >= self.s.len() {
                return Err(self.error(&format!("unterminated element `<{parent}>`")));
            } else {
                let end = self.rest().find('<').unwrap_or(self.rest().len());
                let text = unescape(&self.rest()[..end]).map_err(|e| self.error(&e))?;
                nodes.push(Node::Text(text));
                self.at += end;
            }
        }
    }
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
}

/// Resolves `&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`, `&#N;`, `&#xH;`.
pub fn unescape(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let semi = after.find(';').ok_or_else(|| "unterminated entity".to_string())?;
        out.push_str(&entity(&after[..semi])?);
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn entity(name: &str) -> Result<String, String> {
    let ch = match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let code = match name.strip_prefix('#') {
                Some(hex) if hex.starts_with('x') || hex.starts_with('X') => u32::from_str_radix(&hex[1..], 16).ok(),
                Some(dec) => dec.parse::<u32>().ok(),
                None => None,
            };
            code.and_then(char::from_u32).ok_or_else(|| format!("unknown entity `&{name};`"))?
        }
    };
    Ok(ch.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elements_attributes_text_and_entities() {
        let doc = "<?xml version=\"1.0\"?>\n<a x=\"1 &amp; 2\"><!-- c --><b/><b>t &lt;u&gt; &#65;&#x42;</b><![CDATA[<raw>]]></a>";
        let root = parse(doc).expect("parses");
        assert_eq!(root.name, "a");
        assert_eq!(root.attr("x"), Some("1 & 2"));
        let bs: Vec<&Element> = root.children_named("b").collect();
        assert_eq!(bs.len(), 2);
        assert_eq!(bs[1].text(), "t <u> AB");
        assert_eq!(root.text(), "<raw>");
    }

    #[test]
    fn refuses_mismatched_and_unterminated_tags() {
        assert!(parse("<a></b>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse("<!DOCTYPE x><a/>").is_err());
        assert!(parse("<a>&nope;</a>").is_err());
    }
}
