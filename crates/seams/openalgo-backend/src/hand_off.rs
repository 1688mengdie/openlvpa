#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bind {
    Checked,
    RunnerWindow,
}

pub struct Surface {
    pub name: &'static str,
    pub bind: Bind,
}

pub const SURFACES: &[Surface] = &[
    Surface {
        name: "startup_status",
        bind: Bind::Checked,
    },
    Surface {
        name: "retry_server",
        bind: Bind::Checked,
    },
    Surface {
        name: "restart_server",
        bind: Bind::Checked,
    },
    Surface {
        name: "open_external",
        bind: Bind::Checked,
    },
    Surface {
        name: "reset_account",
        bind: Bind::Checked,
    },
    Surface {
        name: "start_fresh",
        bind: Bind::Checked,
    },
    Surface {
        name: "window_proof",
        bind: Bind::Checked,
    },
    Surface {
        name: "restart_to_update",
        bind: Bind::Checked,
    },
];

pub fn surface(name: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|surface| surface.name == name)
}

pub fn permitted(name: &str, runner_window: bool) -> bool {
    match surface(name) {
        Some(surface) if surface.bind == Bind::RunnerWindow => runner_window,
        Some(_) => true,
        None => false,
    }
}

pub struct Seam {
    pub next: Box<dyn Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync>,
    pub refused: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Seam {
    pub fn call(&self, invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
        let runner_window = invoke.message.webview().window().label() == "runner";
        if !permitted(invoke.message.command(), runner_window) {
            let command = invoke.message.command().to_string();
            match self.refused.lock() {
                Ok(mut refused) => refused.push(command.clone()),
                Err(poisoned) => poisoned.into_inner().push(command.clone()),
            }
            invoke.resolver.reject(format!(
                "the runner window is not granted that surface: {command}"
            ));
            eprintln!(
                "[lvpa-seam] refused {command}: reached from a window the surface does not bind"
            );
            return true;
        }
        (self.next)(invoke)
    }
}
