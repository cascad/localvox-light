//! Windows audio calls share CPAL's process-wide WASAPI enumerator.
//! Its COM apartment must outlive request/capture threads (RustAudio/cpal#1302).
use anyhow::{anyhow, Context, Result};
use std::{cell::RefCell, marker::PhantomData, sync::Mutex};
use windows::{
    core::Owned,
    Win32::System::Com::{
        CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
        CO_MTA_USAGE_COOKIE,
    },
};

struct ProcessMta {
    _cookie: Owned<CO_MTA_USAGE_COOKIE>,
}

// SAFETY: this is an MTA usage token, not an apartment-bound COM interface.
// Microsoft explicitly permits increment/decrement on different threads:
// https://learn.microsoft.com/windows/win32/api/combaseapi/nf-combaseapi-coincrementmtausage
unsafe impl Send for ProcessMta {}

// CPAL keeps its enumerator in a static. Match that process lifetime: releasing
// the MTA between requests would leave its cached interface dangling. Rust statics
// are not dropped at shutdown; the OS reclaims this single process-scoped token.
static PROCESS: Mutex<Option<ProcessMta>> = Mutex::new(None);

struct ThreadApartment(PhantomData<*mut ()>);

impl Drop for ThreadApartment {
    fn drop(&mut self) {
        // SAFETY: created only after successful CoInitializeEx on this thread;
        // the TLS guard is !Send and balances that call once, including S_FALSE.
        unsafe { CoUninitialize() };
    }
}

thread_local! {
    static APARTMENT: RefCell<Option<ThreadApartment>> = const { RefCell::new(None) };
}

pub(super) fn prepare() -> Result<()> {
    {
        let mut process = PROCESS
            .lock()
            .map_err(|_| anyhow!("Windows audio MTA initialization lock poisoned"))?;
        if process.is_none() {
            // SAFETY: no raw COM interfaces are exposed; the returned cookie is
            // uniquely owned by this process-wide guard. Failed init is retryable.
            let cookie =
                unsafe { CoIncrementMTAUsage() }.context("keeping Windows audio MTA alive")?;
            *process = Some(ProcessMta {
                _cookie: unsafe { Owned::new(cookie) },
            });
        }
    }
    APARTMENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            // SAFETY: this initializes only the calling thread. An existing STA
            // is an error: do not let CPAL create/cache an interface in that STA.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .context("Windows audio requires an MTA worker thread")?;
            *slot = Some(ThreadApartment(PhantomData));
        }
        Ok(())
    })
}
