pub fn error_from_win32() -> windows_core::Error {
    windows_core::Error::from_thread()
}
