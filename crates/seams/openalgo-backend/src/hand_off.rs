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
        bind: Bind::RunnerWindow,
    },
    Surface {
        name: "restart_to_update",
        bind: Bind::Checked,
    },
];

pub fn surface(name: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|surface| surface.name == name)
}

pub fn permitted(name: &str, window: &str) -> bool {
    match surface(name) {
        Some(surface) if surface.bind == Bind::RunnerWindow => window == "runner",
        Some(_) => true,
        None => false,
    }
}

pub fn refuse(command: &str, window: &str) -> Option<String> {
    match permitted(command, window) {
        true => None,
        false => Some(format!(
            "the window {window} is not granted the surface {command}"
        )),
    }
}

pub mod seam {
    use super::permitted;

    pub struct Seam<N> {
        pub window_proof: N,
    }

    pub fn open<N>(held: Seam<N>) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static
    where
        N: Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
    {
        move |invoke| {
            let command = invoke.message.command().to_string();
            let window = invoke.message.webview().window().label().to_string();
            if permitted(&command, &window) {
                return (held.window_proof)(invoke);
            }
            invoke.resolver.reject("the window is not granted this surface");
            true
        }
    }
}
