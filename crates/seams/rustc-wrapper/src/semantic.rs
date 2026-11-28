pub struct Nest {
    pub envelope: &'static str,
    pub entry: &'static str,
}

pub struct Composition {
    pub text: String,
    pub appended: bool,
}

pub enum Nesting {
    Landed(Composition),
    NoSuchSeam,
    AnchorGone(&'static str),
}

pub fn nest(text: &str, file: &str, seam: &Nest) -> Nesting {
    let parsed = match syn::parse_file(text) {
        Ok(parsed) => parsed,
        Err(_) => return Nesting::AnchorGone("the file it is written for no longer parses"),
    };
    let Some(envelope) = declared(&parsed, seam.envelope) else {
        return Nesting::AnchorGone("its envelope is no longer declared here, so the seam has no body to carry");
    };
    if !carries(&envelope.block, seam.entry) {
        return Nesting::AnchorGone(
            "its envelope no longer resolves the entry the seam replaces, so wrapping it would hand the window no command at all",
        );
    }

    let opening = envelope.block.brace_token.span.open();
    let closing = envelope.block.brace_token.span.close();
    let (Some((head, after_open)), Some((_, after_close))) = (
        crate::rules::TextRange::between(opening.start(), opening.end())
            .ok()
            .and_then(|at| at.bytes(text)),
        crate::rules::TextRange::between(closing.start(), closing.end())
            .ok()
            .and_then(|at| at.bytes(text)),
    ) else {
        return Nesting::AnchorGone("the braces of its envelope cannot be turned into a position");
    };
    if head >= after_close || &text[head..after_open] != "{" {
        return Nesting::AnchorGone("the braces of its envelope cannot be turned into a position");
    }
    let body = match std::fs::read_to_string(file) {
        Ok(body) if !body.trim().is_empty() => body,
        Ok(_) => return Nesting::AnchorGone("the seam body it carries is empty"),
        Err(_) => return Nesting::AnchorGone("the seam body it carries cannot be read"),
    };
    let Some(site) = held(&body, seam.entry) else {
        return Nesting::AnchorGone(
            "its body declares no type of the envelope it lands in, so the window would be handed a call it cannot make",
        );
    };

    let inner = indent_at(text, head);
    let entry = envelope.sig.ident.to_string();
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(text.len() + body.len() + site.len());
    out.push_str(&text[..head]);
    out.push_str("{\n");
    for line in body.replace("\r\n", "\n").lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(&inner);
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&entry);
    out.push_str("();");
    out.push_str(newline);
    out.push_str(&site);
    out.push_str(&text[after_close + 2..]);
    let _ = after_open;
    Nesting::Landed(Composition {
        text: out,
        appended: true,
    })
}

fn held(body: &str, entry: &str) -> Option<String> {
    let parsed = syn::parse_file(body).ok()?;
    let module = declaring_module(&parsed.items)?;
    let ready = module.ident.to_string();
    let (_, inner) = module.content.as_ref()?;
    let fields = declared_fields(inner, SEAM_TYPE)?;
    let passes = opening_into_the_entry(inner, entry)?;
    let mut out = format!("{ready}::{passes}({ready}::{SEAM_TYPE} {{\n");
    for field in fields {
        out.push_str("        ");
        out.push_str(&field);
        out.push_str(",\n");
    }
    out.push_str("    })\n");
    Some(out)
}

fn opening_into_the_entry(items: &[syn::Item], entry: &str) -> Option<String> {
    for item in items {
        let syn::Item::Fn(function) = item else {
            continue;
        };
        if function.sig.ident == "open" && calls(&function.block, entry) {
            return Some(function.sig.ident.to_string());
        }

    }
    None
}

fn carries(block: &syn::Block, entry: &str) -> bool {
    carried_inner(block, entry)
}

fn calls(block: &syn::Block, entry: &str) -> bool {
    carried_inner(block, entry)
}

fn carried_inner(block: &syn::Block, entry: &str) -> bool {
    let mut spilled = proc_macro2::TokenStream::new();
    for statement in &block.stmts {
        spilled.extend(tokens(statement));
    }
    named(spilled, entry)
}



fn tokens(statement: &syn::Stmt) -> Vec<proc_macro2::TokenTree> {
    let mut out = Vec::new();
    match statement {
        syn::Stmt::Expr(expression, _) => spill(expression, &mut out),
        syn::Stmt::Local(local) => {
            if let Some(init) = &local.init {
                spill(&init.expr, &mut out);
            }
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
    }
    out
}

fn spill(expression: &syn::Expr, out: &mut Vec<proc_macro2::TokenTree>) {
    use proc_macro2::{Delimiter, Group, TokenTree};
    match expression {
        syn::Expr::Call(call) => {
            spill(&call.func, out);
            for argument in &call.args {
                spill(argument, out);
            }
        }
        syn::Expr::Closure(closure) => {
            let mut inner: proc_macro2::TokenStream = tokens_in(&closure.body).into_iter().collect();
            out.push(TokenTree::Group(Group::new(Delimiter::Brace, std::mem::take(&mut inner))));
        }
        syn::Expr::Match(matching) => {
            spill(&matching.expr, out);
            for arm in &matching.arms {
                let mut inner: proc_macro2::TokenStream = tokens_in(&arm.body).into_iter().collect();
                out.push(TokenTree::Group(Group::new(Delimiter::Brace, std::mem::take(&mut inner))));
            }
        }
        syn::Expr::Block(block) => {
            for statement in &block.block.stmts {
                out.extend(tokens(statement));
            }
        }
        syn::Expr::If(conditional) => {
            spill(&conditional.cond, out);
            let mut then = proc_macro2::TokenStream::new();
            for statement in &conditional.then_branch.stmts {
                then.extend(tokens(statement));
            }
            out.push(TokenTree::Group(Group::new(Delimiter::Brace, then)));
            if let Some((_, otherwise)) = &conditional.else_branch {
                spill(otherwise, out);
            }
        }
        syn::Expr::MethodCall(call) => spill(&call.receiver, out),
        syn::Expr::Try(trying) => spill(&trying.expr, out),
        syn::Expr::Paren(paren) => spill(&paren.expr, out),
        syn::Expr::Unary(unary) => spill(&unary.expr, out),
        syn::Expr::Reference(reference) => spill(&reference.expr, out),
        syn::Expr::Field(field) => {
            spill(&field.base, out);
            if let syn::Member::Named(name) = &field.member {
                out.push(TokenTree::Ident(name.clone()));
            }
        }
        syn::Expr::Return(returning) => {
            if let Some(value) = &returning.expr {
                spill(value, out);
            }
        }
        syn::Expr::Await(awaiting) => spill(&awaiting.base, out),
        syn::Expr::Path(path) => {
            if let Some(segment) = path.path.segments.last() {
                out.push(TokenTree::Ident(segment.ident.clone()));
            }
        }
        syn::Expr::Macro(mac) => {
            out.push(TokenTree::Group(Group::new(Delimiter::Brace, mac.mac.tokens.clone())));
        }
        _ => {}
    }
}

fn tokens_in(expression: &syn::Expr) -> Vec<proc_macro2::TokenTree> {
    let mut out = Vec::new();
    spill(expression, &mut out);
    out
}





fn named(stream: proc_macro2::TokenStream, entry: &str) -> bool {
    for token in stream {
        match token {
            proc_macro2::TokenTree::Ident(ident) => {
                if ident == entry {
                    return true;
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                if named(group.stream(), entry) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

const SEAM_TYPE: &str = "Seam";

fn declared(file: &syn::File, name: &str) -> Option<syn::ItemFn> {
    file.items.iter().find_map(|item| match item {
        syn::Item::Fn(function) if function.sig.ident == name => Some(function.clone()),
        _ => None,
    })
}

fn declaring_module(items: &[syn::Item]) -> Option<&syn::ItemMod> {
    items.iter().find_map(|item| match item {
        syn::Item::Mod(module) if module.content.is_some() => Some(module),
        _ => None,
    })
}

fn declared_fields(items: &[syn::Item], wanted: &str) -> Option<Vec<String>> {
    items.iter().find_map(|item| match item {
        syn::Item::Struct(block) if block.ident == wanted => Some(
            block
                .fields
                .iter()
                .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
                .collect(),
        ),
        _ => None,
    })
}

fn indent_at(text: &str, brace: usize) -> String {
    let bytes = text.as_bytes();
    let mut at = brace;
    while at > 0 {
        at -= 1;
        if bytes[at] == b'\n' {
            let mut end = at + 1;
            while end < brace && (bytes[end] == b' ' || bytes[end] == b'\t') {
                end += 1;
            }
            return text[at + 1..end].to_string();
        }
    }
    String::new()
}
