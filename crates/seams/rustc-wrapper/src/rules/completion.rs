use syn::visit::Visit;

use super::{Landed, RuleUnit, TextRange};

pub struct Completion {
    pub seam: &'static str,
    pub exports: &'static str,
    pub trait_name: &'static str,
    pub method: &'static str,
    pub awaited: &'static str,
}

impl Completion {
    pub fn unit(&self) -> RuleUnit {
        RuleUnit::Library
    }
}

pub struct Call {
    pub module: String,
    pub trait_name: String,
    pub method: String,
}

fn is_wait(call: &syn::ExprMethodCall, awaited: &str) -> bool {
    call.method == awaited && call.args.is_empty() && call.turbofish.is_none()
}

struct Waits<'a> {
    awaited: &'a str,
    sites: Vec<&'a syn::ExprMethodCall>,
}

impl<'ast> Visit<'ast> for Waits<'ast> {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if is_wait(call, self.awaited) {
            self.sites.push(call);
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn waits<'a>(check: &Completion, file: &'a syn::File) -> Vec<&'a syn::ExprMethodCall> {
    let mut seen = Waits {
        awaited: check.awaited,
        sites: Vec::new(),
    };
    seen.visit_file(file);
    seen.sites
}

fn declared_method(trait_item: &syn::ItemTrait, wanted: &str) -> Option<String> {
    if trait_item.ident != wanted {
        return None;
    }
    let mut declared: Vec<String> = Vec::new();
    for item in &trait_item.items {
        if let syn::TraitItem::Fn(function) = item {
            declared.push(function.sig.ident.to_string());
        }
    }
    match declared.len() {
        1 => declared.pop(),
        _ => None,
    }
}

fn declared_trait(items: &[syn::Item], wanted: &str) -> Option<String> {
    for item in items {
        match item {
            syn::Item::Trait(block) => {
                if let Some(method) = declared_method(block, wanted) {
                    return Some(method);
                }
            }
            syn::Item::Mod(module) => match &module.content {
                Some((_, inner)) => {
                    if let Some(method) = declared_trait(inner, wanted) {
                        return Some(method);
                    }
                }
                None => continue,
            },
            _ => continue,
        }
    }
    None
}

fn expected_method(completion: &Completion) -> Result<String, String> {
    let parsed = read_seam_body(completion)?;
    let method = declared_trait(&parsed.items, completion.trait_name).ok_or_else(|| {
        format!(
            "{} declares no trait {} across exactly one method in {}, so the method this seam replaces is not the one it declares",
            completion.seam, completion.trait_name, completion.exports
        )
    })?;
    match method == completion.method {
        true => Ok(method),
        false => Err(format!(
            "{} declares {} in {}, and the rule replaces {}::{}",
            completion.seam, method, completion.exports, completion.trait_name, completion.method
        )),
    }
}

fn read_seam_body(completion: &Completion) -> Result<syn::File, String> {
    let root = crate::sources::workspace_root().ok_or_else(|| {
        "the workspace root could not be resolved, so the seam body it declares cannot be read".to_string()
    })?;
    let body = std::fs::read_to_string(root.join(completion.exports)).map_err(|error| {
        format!(
            "{} is not readable as the seam body {} declares: {error}",
            completion.exports, completion.seam
        )
    })?;
    syn::parse_file(&body).map_err(|error| {
        format!(
            "{} does not parse as the seam body {} declares: {error}",
            completion.exports, completion.seam
        )
    })
}

pub fn declares(completion: &Completion) -> Result<(), String> {
    expected_method(completion).map(|_| ())
}

fn awaits(exports: &syn::File, trait_name: &str) -> bool {
    let mut out = Vec::new();
    collect_impls(&exports.items, &mut out);
    out.iter().any(|reached| reached == trait_name)
}

fn call_of(parsed: &syn::File, completion: &Completion) -> Result<Call, String> {
    let method = declared_trait(&parsed.items, completion.trait_name).ok_or_else(|| {
        format!(
            "{} declares no trait {} across exactly one method in {}, so the method this seam replaces is not the one it declares",
            completion.seam, completion.trait_name, completion.exports
        )
    })?;
    Ok(Call {
        module: completion.seam.trim_start_matches("lvpa_").to_string(),
        trait_name: completion.trait_name.to_string(),
        method,
    })
}

impl Call {
    pub fn site(&self) -> String {
        format!("::{}::{}::{}(", self.module, self.trait_name, self.method)
    }

    pub fn is_awaited_by(&self, exports: &syn::File) -> bool {
        awaits(exports, &self.trait_name)
    }
}

fn collect_impls(items: &[syn::Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Impl(block) => {
                let Some((_, reached, _)) = &block.trait_ else {
                    continue;
                };
                let Some(segment) = reached.segments.last() else {
                    continue;
                };
                out.push(segment.ident.to_string());
            }
            syn::Item::Mod(module) => {
                if let Some((_, inner)) = &module.content {
                    collect_impls(inner, out);
                }
            }
            _ => {}
        }
    }
}

pub fn wrap(text: &str, completion: &Completion, exports: &str) -> Landed {
    let Ok(file) = syn::parse_file(text) else {
        return Landed::AnchorMissing;
    };
    let sites = waits(completion, &file);
    if sites.is_empty() {
        return Landed::NotApplicable;
    }
    let Ok(parsed) = syn::parse_file(exports) else {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} does not parse as the seam body it is declared to be",
            completion.exports
        );
        return Landed::AnchorMissing;
    };
    let call = match call_of(&parsed, completion) {
        Ok(call) => call,
        Err(missing) => {
            eprintln!("[lvpa-wrapper] FATAL: {missing}");
            return Landed::AnchorMissing;
        }
    };
    if !call.is_awaited_by(&parsed) {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} declares {} in {}, and no impl in that body carries it",
            completion.seam, call.trait_name, completion.exports
        );
        return Landed::AnchorMissing;
    }
    let site = call.site();
    let mut spans: Vec<TextRange> = sites
        .iter()
        .filter_map(|call| TextRange::of_call(call).ok())
        .collect();
    if spans.is_empty() {
        return Landed::AnchorMissing;
    }
    spans.sort();
    let mut out = String::with_capacity(text.len() + spans.len() * (site.len() + 16));
    let mut cursor = 0usize;
    for span in spans {
        if !span.after(cursor) {
            return Landed::AnchorMissing;
        }
        let Some((start, end)) = span.bytes(text) else {
            return Landed::AnchorMissing;
        };
        out.push_str(&text[cursor..start]);
        out.push_str("w(");
        out.push_str(completion.seam);
        out.push_str(&site);
        out.push_str(&text[start..end]);
        out.push(')');
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Landed::Rewritten(out)
}
