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

/// A call written at one shape and landed at another: the shape the source writes is the
/// name the rule holds a call to, and the entry the same call is rewritten to is the name
/// this repository hands the driver instead.
pub struct Surround {
    pub from: &'static str,
    pub to: &'static str,
    pub seam: &'static str,
    /// The entry a call whose text the program does not state enters through, when the shape has
    /// one: the arguments the call carries stay as they stand, and the entry names the text.
    pub assembled: Option<&'static str>,
    /// The entry a call states its row type to, when the shape has one: the type the call states
    /// stands beside the statement the entry names, and a call that states no type reaches the
    /// entry the text names.
    pub row: Option<&'static str>,
}

impl Surround {
    pub fn unit(&self) -> RuleUnit {
        RuleUnit::Library
    }

    /// The entry a call whose text the program states enters through.
    fn entry(&self) -> &'static str {
        self.to.strip_suffix('(').unwrap_or(self.to)
    }

    fn tail(&self) -> Vec<&'static str> {
        self.from.strip_suffix("()").unwrap_or(self.from).split("::").collect()
    }
}

struct Seen {
    site: TextRange,
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
        let end = tokens[index].span().end();
        let Ok(site) = TextRange::between(start, end) else {
            return;
        };
        if !self.named.iter().any(|known| known.site == site) {
            self.named.push(Written { site });
        }
    }

    fn record(&mut self, path: &syn::Path) {
        // the name a call states its own arguments at is a segment of the path, a turbofish does
        // not change the name, and a type standing beside it reaches the entry beside the statement
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
            self.written.push(Seen { site });
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

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(entry) = call.func.as_ref() {
            self.record(&entry.path);
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        syn::visit::visit_local(self, local);
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

struct Wrapped {
    tail: &'static [&'static str],
    /// The text the program writes, read where the calls of the shape stand in it.
    text: &'static str,
    /// The width of a call of the shape that states no text: the name, the type it states, and the
    /// parenthesis the name opens the call at.
    named_width: usize,
    /// The place each call of the shape stands at, and whether the call writes its statement out.
    written: Vec<(TextRange, bool)>,
    assembled: Vec<TextRange>,
    /// The type a call states, held at the place the call stands: the site is the call's own
    /// span, so a place where two calls meet reads the type each of them states
    rows: Vec<(TextRange, String)>,
}

impl Wrapped {
    /// The type a call states, read off the path it is written by: a call states it in a turbofish
    /// the shape's own name stands in, so only the segment the shape names is read, and the type is
    /// held as the program wrote it.
    fn row_of(&self, path: &syn::Path) -> Option<String> {
        let segment = path
            .segments
            .iter()
            .zip(self.tail.iter())
            .find(|(segment, name)| segment.ident == **name)?;
        let syn::PathArguments::AngleBracketed(arguments) = &segment.0.arguments else {
            return None;
        };
        let mut type_only = arguments.args.iter().filter(|argument| matches!(argument, syn::GenericArgument::Type(_)));
        let syn::GenericArgument::Type(row) = type_only.next()? else {
            return None;
        };
        let spelled: proc_macro2::TokenStream = syn::parse_quote! { #row };
        Some(spelled.to_string())
    }
}

impl Wrapped {
    /// Whether the run of names a call reaches is the run the shape names.
    fn names(&self, names: Vec<String>) -> bool {
        names.iter().map(String::as_str).eq(self.tail.iter().copied())
    }

    /// A call of the shape is the shape rewritten to the entry: the path the source writes
    /// the name down at is the site, and every argument the call carries stays as it stands.
    fn covering(&mut self, call: &syn::ExprCall) {
        let callee = match call.func.as_ref() {
            syn::Expr::Path(path) => path,
            _ => return,
        };
        // a segment that states its own arguments states the row type the call returns, so the
        // name the shape matches is the name the call reaches, and the type stands beside it
        let called = path_without_its_arguments(&callee.path);
        let says = self.names(called);
        if !says {
            return;
        }
        let site = match parenthesis_after(&callee.path, self.text) {
            Ok(site) => site,
            Err(_) => return,
        };
        let stated = call
            .args
            .first()
            .is_some_and(|argument| matches!(argument, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(_), .. })));
        // the place a call of the shape stands at is the run of names it reaches, and a segment
        // that states its own arguments states the row type the call returns, so the width the
        // place keeps is the width of the names without the arguments one call of the shape states
        let width = match site.bytes(self.text) {
            Some((start, end)) if end - start >= self.named_width => self.named_width,
            _ => self.named_width,
        };
        let site = site.widened(width);
        if !self.written.iter().any(|known| known.0 == site) {
            self.written.push((site, stated));
            if let Some(row) = self.row_of(&callee.path) {
                self.rows.push((site, row));
            }
        }
        // the text a call carries is the text the call states after the name
        if let Some(argument) = call.args.first() {
            if stated {
                return;
            }
            if let Ok(held) = TextRange::between(argument.span().start(), argument.span().end()) {
                if !self.assembled.iter().any(|known| *known == held) {
                    self.assembled.push(held);
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Wrapped {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        self.covering(call);
        syn::visit::visit_expr_call(self, call);
    }
}

/// A file walked with one block of it held back: the item the attribute is written on is the item
/// whose statements the rule does not reach, and every other item of the file is read as it stands.
struct Holding<'a> {
    inner: &'a mut Wrapped,
    attribute: &'a str,
    held: bool,
}

impl<'ast> Visit<'ast> for Holding<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let carries = match item {
            syn::Item::Mod(module) => {
                module.attrs.iter().any(|attribute| attribute.path().is_ident(self.attribute))
            }
            syn::Item::Fn(function) => {
                function.attrs.iter().any(|attribute| attribute.path().is_ident(self.attribute))
            }
            _ => false,
        };
        if carries {
            let outside = self.held;
            self.held = true;
            syn::visit::visit_item(self, item);
            self.held = outside;
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.held {
            return;
        }
        self.inner.covering(call);
        syn::visit::visit_expr_call(self, call);
    }
}

/// The place the parenthesis that opens a call stands at: the name ends where the parenthesis the
/// call is written by opens, and the parenthesis is the first one the text after the name carries.
fn parenthesis_after(path: &syn::Path, text: &str) -> Result<TextRange, String> {
    let end = path.span().end();
    let mut at = 0usize;
    for _ in 1..end.line {
        at += text
            .get(at..)
            .and_then(|rest| rest.find('\n'))
            .ok_or_else(|| "a seam site names a line the text it stands in does not carry".to_string())?
            + 1;
    }
    let opening = text
        .get(at + end.column..)
        .and_then(|rest| rest.find('('))
        .ok_or_else(|| "a call of the shape opens no parenthesis after its name".to_string())?;
    TextRange::between(
        end,
        proc_macro2::LineColumn {
            line: end.line,
            column: end.column + opening + 1,
        },
    )
}

/// What the calls of a shape state in a text: the places the calls stand at, and the entry each of
/// them is landed at. A statement the source writes out is read by the driver as its own text, and a
/// statement the source assembles at run time is not, so the call that carries the text itself is
/// landed at the entry that marks it, and the call that carries a binding is landed at the entry that
/// takes the text as it stands.
struct Places {
    /// The place each call stands at, the entry it is landed at, and the line the source writes it
    /// on once the line the compiler reads has been joined into the line the source continues.
    sites: Vec<(usize, usize, &'static str, usize)>,
}

fn places_of(written: &str, shape: &Surround) -> Option<Places> {
    let file = match syn::parse_file(written) {
        Ok(file) => file,
        Err(error) => {
            let point = error.span().start();
            eprintln!("[lvpa-wrapper] unreadable {} at {}+{}: {}", shape.from, point.line, point.column, error);
            return None;
        }
    };
    places_beside(written, shape, &file, None)
}

/// The places a rule holds back from: every call of the shape stands outside the block the file
/// opens with the attribute, and the block is the one the attribute is written on.
fn places_outside(written: &str, shape: &Surround, attribute: &str) -> Option<Places> {
    let file = syn::parse_file(written).ok()?;
    places_beside(written, shape, &file, Some(attribute))
}

fn places_beside(written: &str, shape: &Surround, file: &syn::File, outside: Option<&str>) -> Option<Places> {
    let mut shapes = Wrapped {
        tail: Box::leak(shape.tail().into_boxed_slice()),
        text: Box::leak(written.to_owned().into_boxed_str()),
        named_width: shape.tail().iter().map(|name| name.len()).sum::<usize>() + 3,
        assembled: Vec::new(),
        written: Vec::new(),
        rows: Vec::new(),
    };
    match outside {
        Some(attribute) => {
            let mut held = Holding {
                inner: &mut shapes,
                attribute,
                held: false,
            };
            syn::visit::Visit::visit_file(&mut held, file);
        }
        None => syn::visit::Visit::visit_file(&mut shapes, file),
    }
    if shapes.written.is_empty() {
        return None;
    }
    let mut carried = 0usize;
    let sites: Vec<(usize, usize, &'static str, usize)> = shapes
        .written
        .iter()
        .filter_map(|(place, stated)| match place.bytes(written) {
            Some(held) => {
                carried += 1;
                let entry = match (shape.assembled, stated) {
                    (Some(assembled), false) => assembled,
                    _ => shape.entry(),
                };
                Some((held.0, held.1, entry, place.line() + 1))
            }
            None => None,
        })
        .collect();
    if carried != shapes.written.len() || sites.is_empty() {
        return None;
    }
    Some(Places { sites })
}

/// The text a rule reads: a line the source continues into the line below it is one line for the
/// compiler, and the join keeps every line on its own line, so a place reads the same line in both
/// texts and the column the join moved it to is the column the compiler states it at.
fn reads_the_file(source: &str) -> Option<String> {
    let mut written = String::with_capacity(source.len());
    for (at, line) in source.split('\n').enumerate() {
        let last = at + 1 == source.split('\n').count();
        let body = line.strip_suffix('\r').unwrap_or(line);
        match body.strip_suffix('\\') {
            Some(carried) => written.push_str(carried),
            None => written.push_str(body),
        }
        if !(body.ends_with('\\') || last) {
            written.push('\n');
        }
    }
    Some(written)
}

/// The byte the source writes a place of the text the compiler reads: the text the compiler reads
/// holds a run of continued lines as one line, so a place walks the lines the source writes, every
/// line but the last carrying the line below it, and the width a line takes there is the line less
/// the mark it carries into the line below.
fn byte_of(text: &str, at: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut kept = at;
    let mut reached = 0usize;
    for written in lines.iter() {
        let width = written.len() - usize::from(written.ends_with('\\'));
        if kept < width || (kept == width && !written.ends_with('\\')) {
            return Some((reached + kept, width - kept));
        }
        kept = kept.checked_sub(width + 1)?;
        reached += written.len() + 1;
    }
    None
}

/// The text a file writes with every call of the shape landed at its entry: the place of a call is
/// the run of names it reaches, read on the text the compiler reads, and it is written out where the
/// source writes it, so every line keeps its own ending and the byte a call stands at is the byte the
/// compiler states it at.
fn rewrite(text: &str, places: &Places) -> Option<String> {
    let mut found: Vec<(usize, usize, &'static str)> = places
        .sites
        .iter()
        .map(|(start, end, entry, _)| {
            let (at, _) = byte_of(text, *start)?;
            Some((at, at + (end - start), *entry))
        })
        .collect::<Option<Vec<_>>>()?;
    found.sort_unstable_by_key(|(start, _, _)| *start);
    let mut out = String::with_capacity(text.len() + found.len() * 8);
    let mut reached = 0usize;
    for (start, end, entry) in found {
        if start < reached || start > end || end > text.len() {
                return None;
        }
        out.push_str(text.get(reached..start)?);
        out.push_str(entry);
        reached = end;
    }
    out.push_str(text.get(reached..)?);
    Some(out)
}

pub fn surround(text: &str, shape: &Surround) -> Landed {
    if segments_of(shape.from).is_none() || segments_of(shape.to).is_none() {
        return Landed::AnchorMissing;
    }
    let Some(written) = reads_the_file(text) else {
        return Landed::AnchorMissing;
    };
    let Some(places) = places_of(&written, shape) else {
        return Landed::AnchorMissing;
    };
    let landed = rewrite(text, &places);
    match landed {
        Some(rewritten) => Landed::Rewritten(rewritten),
        None => Landed::AnchorMissing,
    }
}

/// A shape landed everywhere but inside the block the file opens with the attribute.
pub fn surround_outside(text: &str, shape: &Surround, attribute: &str) -> Landed {
    if segments_of(shape.from).is_none() || segments_of(shape.to).is_none() {
        return Landed::AnchorMissing;
    }
    let Some(written) = reads_the_file(text) else {
        return Landed::AnchorMissing;
    };
    let Some(places) = places_outside(&written, shape, attribute) else {
        return Landed::AnchorMissing;
    };
    match rewrite(text, &places) {
        Some(rewritten) => Landed::Rewritten(rewritten),
        None => Landed::AnchorMissing,
    }
}

/// A name is rewritten where the source writes it: the name the rule holds is a run of names, and
/// the run is replaced by the run the rule lands, with everything the source writes around it kept.
pub fn rename(text: &str, rename: &Rename) -> Landed {
    let Ok(file) = syn::parse_file(text) else {
        return Landed::NotApplicable;
    };
    let Some(tail) = segments_of(rename.from) else {
        return Landed::AnchorMissing;
    };
    let Some(head) = segments_of(rename.to) else {
        return Landed::AnchorMissing;
    };
    let width = tail.len().saturating_mul(2).saturating_sub(1);
    let mut seats = Sees {
        tail: Box::leak(tail.into_boxed_slice()),
        head: head.last().copied(),
        width,
        written: Vec::new(),
        named: Vec::new(),
    };
    syn::visit::Visit::visit_file(&mut seats, &file);
    let mut spans: Vec<(usize, usize)> = seats.named.iter().filter_map(|one| one.site.bytes(text)).collect();
    if spans.is_empty() {
        return Landed::AnchorMissing;
    }
    spans.sort_unstable();
    spans.dedup();
    let mut out = String::with_capacity(text.len() + spans.len() * rename.to.len());
    let mut cursor = 0usize;
    for (start, end) in spans {
        if start < cursor || start >= end || end > text.len() {
            return Landed::AnchorMissing;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(rename.to.strip_suffix('(').unwrap_or(rename.to));
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Landed::Rewritten(out)
}

fn segments_of(shape: &str) -> Option<Vec<&str>> {
    // the shape names its item by a call the source writes, and the opening parenthesis of that
    // call stands after the name: the path is the name before it
    let shape = shape.strip_suffix("()").unwrap_or(shape);
    let segments: Vec<&str> = shape.split("::").collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some(segments)
}

/// The path a call is written as, read off the syntax: a call that states its own arguments states
/// them in a segment, and the name the shape matches is the path without them.
fn path_without_its_arguments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}
