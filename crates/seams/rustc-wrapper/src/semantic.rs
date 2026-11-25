pub struct Nest {
    pub envelope: &'static str,
    pub hand_off: &'static str,
    pub appended: &'static str,
}

pub struct Composition {
    pub text: String,
    pub appended: bool,
}

pub fn nest(text: &str, file: &str, seam: &Nest) -> Option<Composition> {
    let parsed = syn::parse_file(text).ok()?;
    let envelope = find(&parsed, seam.envelope)?;
    if declares(&envelope.block, seam.hand_off) {
        return Some(Composition {
            text: text.to_string(),
            appended: false,
        });
    }

    let start = offset(text, envelope.block.brace_token.span.open())?;
    let end = offset(text, envelope.block.brace_token.span.close())?;
    if end <= start || text[start..start + 1] != *"{" {
        return None;
    }

    let body = std::fs::read_to_string(file).ok()?;
    if body.trim().is_empty() {
        return None;
    }

    let line_start = text[..start].rfind('\n').map(|at| at + 1).unwrap_or(0);
    let indent: String = text[line_start..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let inner = format!("{indent}    ");

    let mut out = String::with_capacity(text.len() + body.len() + 256);
    out.push_str(&text[..start + 1]);
    out.push('\n');
    out.push_str(&inner);
    out.push_str("mod ");
    out.push_str(seam.hand_off);
    out.push_str(" {\n");
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
    out.push_str("}\n");
    out.push_str(&inner);
    out.push_str("let ");
    out.push_str(seam.appended);
    out.push_str(" = &");
    out.push_str(seam.hand_off);
    out.push_str("::Seam {\n");
    out.push_str(&inner);
    out.push_str("    next: Box::new(handler()),\n");
    out.push_str(&inner);
    out.push_str("    refused: Default::default(),\n");
    out.push_str(&inner);
    out.push_str("};\n");
    out.push_str(&indent);
    out.push_str("move |invoke| ");
    out.push_str(seam.appended);
    out.push_str(".call(invoke)\n");
    out.push_str(&text[end..]);
    Some(Composition {
        text: out,
        appended: true,
    })
}

fn find(file: &syn::File, name: &str) -> Option<syn::ItemFn> {
    for item in &file.items {
        if let syn::Item::Fn(function) = item {
            if function.sig.ident == name {
                return Some(function.clone());
            }
        }
    }
    None
}

fn declares(block: &syn::Block, module: &str) -> bool {
    block.stmts.iter().any(|statement| match statement {
        syn::Stmt::Item(syn::Item::Mod(declared)) => declared.ident == module,
        _ => false,
    })
}

fn offset(text: &str, span: proc_macro2::Span) -> Option<usize> {
    let line = span.start().line;
    let column = span.start().column;
    if line == 0 {
        return None;
    }
    let mut offset = 0usize;
    let mut current = 1usize;
    while current < line {
        offset += text[offset..].find('\n')? + 1;
        current += 1;
    }
    Some((offset + column).min(text.len()))
}
