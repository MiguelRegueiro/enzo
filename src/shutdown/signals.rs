use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
}

#[cfg(unix)]
pub(crate) fn install_signal_handlers() -> io::Result<()> {
    SHUTDOWN_REQUESTED.store(false, Ordering::Relaxed);
    unsafe {
        install_handler(libc::SIGHUP)?;
        install_handler(libc::SIGINT)?;
        install_handler(libc::SIGTERM)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn install_signal_handlers() -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
unsafe fn install_handler(signal: libc::c_int) -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handle_signal as *const () as usize;
    action.sa_flags = 0;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, std::ptr::null_mut()) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}
