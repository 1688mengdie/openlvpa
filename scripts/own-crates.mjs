export const OWN_CRATES = [
  'crates/seams/rustc-wrapper',
  'crates/seams/windows-future',
  'crates/seams/desktop-win32-thread',
  'crates/seams/openalgo-backend',
  'crates/wiring/shape',
];

export const SEAM_CRATES = OWN_CRATES.filter((crate) => crate.startsWith('crates/seams/'));
