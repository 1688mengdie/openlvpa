use syn::{Item, ReturnType, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Composition {
    Envelope {
        host: &'static str,
        envelope: &'static str,
        seam: &'static str,
        entry: &'static str,
    },
}

impl Composition {
    pub fn unit(&self) -> crate::rules::RuleUnit {
        crate::rules::RuleUnit::Library
    }
}

pub struct Command {
    pub name: String,
    pub argument: String,
    pub output: String,
}

pub fn commands(text: &str) -> Option<Vec<Command>> {
    let file = syn::parse_file(text).ok()?;
    let mut out = Vec::new();
    for item in &file.items {
        let Item::Fn(function) = item else {
            continue;
        };
        if !is_command(function) {
            continue;
        }
        out.push(Command {
            name: function.sig.ident.to_string(),
            argument: command_argument(function)?,
            output: command_output(&function.sig.output)?,
        });
    }
    Some(out)
}

fn is_command(function: &syn::ItemFn) -> bool {
    for attribute in &function.attrs {
        if !attribute.path().is_ident("tauri") {
            continue;
        }
        let parsed = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        );
        let Ok(nested) = parsed else {
            continue;
        };
        if nested.iter().any(|meta| meta.path().is_ident("command")) {
            return true;
        }
    }
    false
}

fn command_argument(function: &syn::ItemFn) -> Option<String> {
    let mut typed = Vec::new();
    for input in &function.sig.inputs {
        match input {
            syn::FnArg::Typed(argument) => typed.push(argument),
            syn::FnArg::Receiver(_) => return None,
        }
    }
    match typed.len() {
        0 => Some("()".to_string()),
        1 => argument_type(&typed[0].ty),
        _ => {
            let mut types = String::from("(");
            for argument in typed {
                types.push_str(&argument_type(&argument.ty)?);
                types.push(',');
            }
            types.push(')');
            Some(types)
        }
    }
}

fn command_output(output: &ReturnType) -> Option<String> {
    match output {
        ReturnType::Default => Some("()".to_string()),
        ReturnType::Type(_, ty) => wrapper_and_inner(ty),
    }
}

fn wrapper_and_inner(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    });
    let inner = types.next()?;
    if types.next().is_some() {
        return None;
    }
    argument_type(inner)
}

fn argument_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => last_segment(path),
        Type::Reference(reference) => match reference.elem.as_ref() {
            Type::Path(path) => last_segment(path),
            _ => None,
        },
        Type::Tuple(tuple) if tuple.elems.is_empty() => Some("()".to_string()),
        _ => None,
    }
}

fn last_segment(path: &syn::TypePath) -> Option<String> {
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn const_name(index: usize) -> String {
    format!("COMMAND_{index}")
}

pub fn embed(host: &str, discovered: &[Command]) -> Option<String> {
    let body = std::fs::read_to_string(host).ok()?;
    if body.trim().is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str(&table(discovered));
    out.push('\n');
    out.push_str(&body);
    Some(out)
}

pub fn table(commands: &[Command]) -> String {
    let mut out = String::from(
        "pub struct Command<A, O> {\n    pub name: &'static str,\n    pub argument: fn() -> A,\n    pub output: fn() -> O,\n}\n",
    );
    for (index, command) in commands.iter().enumerate() {
        out.push_str(&format!(
            "pub type Argument{index} = {};\n",
            command.argument
        ));
        out.push_str(&format!("pub type Output{index} = {};\n", command.output));
        out.push_str(&format!(
            "pub const {}: Command<Argument{index}, Output{index}> = Command {{\n    name: \"{}\",\n    argument: || unreachable!(),\n    output: || unreachable!(),\n}};\n",
            const_name(index),
            command.name
        ));
    }
    let entries: Vec<String> = (0..commands.len())
        .map(|index| format!("{}.name", const_name(index)))
        .collect();
    out.push_str(&format!(
        "pub const ENTRIES: &[&'static str] = &[{}];\n",
        entries.join(", ")
    ));
    out
}

pub fn wrap(text: &str, carried: &str, envelope: &'static str, entry: &'static str) -> crate::semantic::Nesting {
    let nest = crate::semantic::Nest {
        envelope,
        entry,
    };
    crate::semantic::nest(text, &carried_path(carried), &nest)
}

fn carried_path(landed: &str) -> String {
    match resolve(landed) {
        Some(path) => path,
        None => landed.to_string(),
    }
}

fn resolve(landed: &str) -> Option<String> {
    let root = match crate::sources::workspace_root() {
        Some(root) => root,
        None => {
            eprintln!(
                "[lvpa-wrapper] FATAL: the workspace root could not be resolved, so the composition body {landed} \
                 cannot be read and nothing tells a rule that carried it from one that did not. Nothing is rewritten."
            );
            std::process::exit(97);
        }
    };
    let path = root.join(landed);
    match path.is_file() {
        true => Some(path.to_string_lossy().into_owned()),
        false => None,
    }
}
