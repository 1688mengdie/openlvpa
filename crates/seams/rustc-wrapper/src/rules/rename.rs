use syn::spanned::Spanned;
use syn::visit::Visit;

use super::{Landed, RuleUnit, TextRange};

pub struct Rename {
    pub from: &'static str,
    pub to: &'static str,
}

impl Rename {
    pub fn unit(&self) -> RuleUnit {
        RuleUnit::Library
    }
}

struct Seen {
    site: TextRange,
    shortens_the_path: bool,
}

struct Written {
    site: TextRange,
}

struct Sees<'a> {
    tail: &'a [&'a str],
    head: Option<&'a str>,
    width: usize,
    written: Vec<Seen>,
    named: Vec<Written>,
}

impl Sees<'_> {
    fn joined(&self) -> String {
        self.tail.join("::")
    }

    fn is_the_tail(&self, names: &[String]) -> bool {
        names.iter().map(String::as_str).eq(self.tail.iter().copied())
    }

    fn names_the_head(&self, names: &[String]) -> bool {
        names.len() == self.tail.len() + 1
            && self.head.is_some_and(|head| names[0] == head)
            && self.is_the_tail(&names[1..])
    }

    fn carries_the_tail(&self, names: &[String]) -> bool {
        names.len() > self.tail.len()
            && self.head.is_some_and(|head| names[names.len() - self.tail.len() - 1] == head)
            && names[names.len() - self.tail.len()..]
                .iter()
                .map(String::as_str)
                .eq(self.tail.iter().copied())
    }

    fn scan_tokens(&mut self, stream: &proc_macro2::TokenStream) {
        let tokens: Vec<proc_macro2::TokenTree> = stream.clone().into_iter().collect();
        if self.width == 0 {
            return;
        }
        let mut index = 0usize;
        while index + self.width <= tokens.len() {
            if self.at_tail(&tokens, index) {
                self.take_tokens(&tokens, index);
                index += self.width;
                continue;
            }
            index += 1;
        }
    }

    fn at_tail(&self, tokens: &[proc_macro2::TokenTree], index: usize) -> bool {
        let tail: String = tokens[index..index + self.width]
            .iter()
            .map(|token| match token {
                proc_macro2::TokenTree::Ident(ident) => ident.to_string(),
                proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':' => ":".to_string(),
                _ => String::new(),
            })
            .collect();
        tail == self.joined()
    }

    fn take_tokens(&mut self, tokens: &[proc_macro2::TokenTree], index: usize) {
        let start = tokens[index].span().start();
        let end = tokens[index + self.width - 1].span().end();
        let Ok(site) = TextRange::between(start, end) else {
            return;
        };
        if !self.named.iter().any(|known| known.site == site) {
            self.named.push(Written { site });
        }
    }

    fn record(&mut self, path: &syn::Path) {
        if path
            .segments
            .iter()
            .any(|segment| !segment.arguments.is_none())
        {
            return;
        }
        let names: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let Some(last) = path.segments.last() else {
            return;
        };
        let Ok(site) = TextRange::between(path.span().start(), last.span().end()) else {
            return;
        };
        let shortest = self.tail.len().min(self.width);
        if !self.written.iter().any(|known| known.site == site) && names.len() >= shortest {
            self.written.push(Seen {
                site,
                shortens_the_path: !self.is_the_tail(&names) && !self.names_the_head(&names) && !self.carries_the_tail(&names),
            });
        }
        if self.is_the_tail(&names) || self.names_the_head(&names) || self.carries_the_tail(&names) {
            if !self.named.iter().any(|known| known.site == site) {
                self.named.push(Written { site });
            }
        }
    }
}

impl<'ast> Visit<'ast> for Sees<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.record(path);
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_macro(&mut self, expr: &'ast syn::ExprMacro) {
        self.scan_tokens(&expr.mac.tokens);
        syn::visit::visit_expr_macro(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.scan_tokens(&mac.tokens);
        syn::visit::visit_macro(self, mac);
    }
}

fn segments_of(shape: &str) -> Option<Vec<&str>> {
    let shape = shape.strip_suffix("()")?;
    let segments: Vec<&str> = shape.split("::").collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some(segments)
}

fn boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    !before.is_some_and(ident) && !after.is_some_and(ident)
}

fn widen(text: &str, end: usize) -> usize {
    let rest = text.get(end..).unwrap_or("");
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some('('), Some(')')) => end + 2,
        _ => end,
    }
}

pub fn rename(text: &str, rename: &Rename) -> Landed {
    let Some(segments) = segments_of(rename.from) else {
        return Landed::AnchorMissing;
    };
    let Ok(file) = syn::parse_file(text) else {
        return Landed::NotApplicable;
    };
    let mut seats = Sees {
        tail: &segments,
        head: segments
            .split_last()
            .and_then(|(_, head)| head.last().copied()),
        width: segments.len() * 2 - 1,
        written: Vec::new(),
        named: Vec::new(),
    };
    seats.visit_file(&file);
    let mut spans: Vec<(usize, usize)> = seats
        .named
        .iter()
        .filter(|written| !seats.written.iter().any(|seen| seen.shortens_the_path && seen.site.contains(&written.site)))
        .filter_map(|written| written.site.bytes(text))
        .map(|(start, end)| (start, widen(text, end)))
        .collect();
    let literal = rename.from;
    let mut search = 0usize;
    while let Some(found) = text[search..].find(literal) {
        let start = search + found;
        let end = start + literal.len();
        search = end;
        if spans
            .iter()
            .any(|(known_start, known_end)| start <= *known_end && *known_start <= end)
        {
            continue;
        }
        if !boundary(text, start, end) {
            continue;
        }
        spans.push((start, end));
    }
    if spans.is_empty() {
        return Landed::AnchorMissing;
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (start, end) in spans {
        if start < cursor || start >= end || end > text.len() {
            return Landed::AnchorMissing;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(rename.to);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Landed::Rewritten(out)
}
