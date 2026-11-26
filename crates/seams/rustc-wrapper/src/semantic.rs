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

    let inner = indent_at(text, head);
    let entry = envelope.sig.ident.to_string();
    let mut out = String::with_capacity(text.len() + body.len() + entry.len());
    out.push_str(&text[..after_open]);
    out.push('\n');
    for line in body.replace("\r\n", "\n").lines() {
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(&inner);
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&inner);
    out.push_str("    ");
    out.push_str(&entry);
    out.push_str("()\n");
    out.push_str(&inner);
    out.push_str(&text[after_close..]);
    Nesting::Landed(Composition {
        text: out,
        appended: true,
    })
}

fn indent_at(text: &str, brace: usize) -> String {
    match text[..brace].rfind('\n') {
        Some(at) => text[at + 1..brace].to_string(),
        None => text[..brace].to_string(),
    }
}

fn declared(file: &syn::File, name: &str) -> Option<syn::ItemFn> {
    file.items.iter().find_map(|item| match item {
        syn::Item::Fn(function) if function.sig.ident == name => Some(function.clone()),
        _ => None,
    })
}

fn carries(block: &syn::Block, entry: &str) -> bool {
    let mut found = false;
    for statement in &block.stmts {
        walk(statement, entry, &mut found);
    }
    found
}

fn walk(statement: &syn::Stmt, entry: &str, found: &mut bool) {
    match statement {
        syn::Stmt::Expr(expression, _) => reached(expression, entry, found),
        syn::Stmt::Local(local) => {
            if let Some(init) = &local.init {
                reached(&init.expr, entry, found);
            }
        }
        _ => {}
    }
}

fn reached(expression: &syn::Expr, entry: &str, found: &mut bool) {
    if *found {
        return;
    }
    match expression {
        syn::Expr::Macro(mac) => {
            if mac.mac.path.segments.last().is_some_and(|segment| segment.ident == "generate_handler") {
                let mut inner = mac.mac.tokens.clone().into_iter();
                while let Some(token) = inner.next() {
                    if let proc_macro2::TokenTree::Ident(ident) = token {
                        if ident == entry {
                            *found = true;
                            return;
                        }
                    }
                }
            }
        }
        syn::Expr::Block(block) => {
            for statement in &block.block.stmts {
                walk(statement, entry, found);
            }
        }
        syn::Expr::Call(call) => reached(&call.func, entry, found),
        syn::Expr::MethodCall(call) => reached(&call.receiver, entry, found),
        syn::Expr::Try(trying) => reached(&trying.expr, entry, found),
        syn::Expr::Paren(paren) => reached(&paren.expr, entry, found),
        syn::Expr::Unary(unary) => reached(&unary.expr, entry, found),
        syn::Expr::Reference(reference) => reached(&reference.expr, entry, found),
        syn::Expr::Field(field) => reached(&field.base, entry, found),
        syn::Expr::Await(awaiting) => reached(&awaiting.base, entry, found),
        _ => {}
    }
}
