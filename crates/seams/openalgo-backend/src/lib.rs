pub mod hand_off;

pub use hand_off::{permitted, surface, Bind, Seam, Surface, SURFACES};

pub fn names() -> Vec<&'static str> {
    SURFACES.iter().map(|surface| surface.name).collect()
}
