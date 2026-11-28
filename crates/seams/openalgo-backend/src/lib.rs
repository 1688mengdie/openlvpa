pub mod hand_off;

use tauri::ipc::Invoke;

pub use hand_off::{permitted, refuse, surface, Bind, Surface, SURFACES};

pub struct Seam {
    pub next: Box<dyn Fn(Invoke<tauri::Wry>) -> bool + Send + Sync>,
    pub refused: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

pub fn open(seam: Seam) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let window = invoke.message.webview().window().label().to_string();
        let command = invoke.message.command().to_string();
        match refuse(&command, &window) {
            Some(said) => {
                match seam.refused.lock() {
                    Ok(mut held) => held.push(command),
                    Err(poisoned) => poisoned.into_inner().push(command),
                }
                invoke.resolver.reject(said);
                true
            }
            None => (seam.next)(invoke),
        }
    }
}
