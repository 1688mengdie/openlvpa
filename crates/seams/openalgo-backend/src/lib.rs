pub mod hand_off;

pub use hand_off::{permitted, surface, Bind, Seam, Surface, SURFACES};

pub fn names() -> Vec<&'static str> {
    SURFACES.iter().map(|surface| surface.name).collect()
}

pub fn open(seam: Seam) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| seam.call(invoke)
}
