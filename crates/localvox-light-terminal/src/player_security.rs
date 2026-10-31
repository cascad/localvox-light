//! Verify the actual named pipe before sending any credentials to it.
use anyhow::{Context, Result};
use std::os::windows::io::AsRawHandle;
use windows::Win32::{
    Foundation::{LocalFree, HANDLE, HLOCAL},
    Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        EqualSid, GetAce, ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    },
    System::Pipes::GetNamedPipeServerProcessId,
};

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

pub fn verify_pipe(pipe: &impl AsRawHandle, expected_pid: u32) -> Result<()> {
    let handle = HANDLE(pipe.as_raw_handle());
    let mut pid = 0;
    // The handle remains owned by Tokio. The descriptor returned by Windows is
    // separately allocated and released exactly once by Descriptor.
    unsafe {
        GetNamedPipeServerProcessId(handle, &mut pid).context("Cannot identify mpv IPC owner")?;
        anyhow::ensure!(
            pid == expected_pid,
            "IPC does not belong to the owned mpv process"
        );
        let mut owner = PSID::default();
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut acl),
            None,
            Some(&mut sd),
        )
        .ok()
        .context("Cannot inspect mpv IPC permissions")?;
        let _descriptor = Descriptor(sd);
        anyhow::ensure!(
            !owner.0.is_null() && !acl.is_null(),
            "IPC has no restricted DACL"
        );
        anyhow::ensure!((*acl).AceCount == 1, "IPC must grant only its owner access");
        let mut ace = std::ptr::null_mut();
        GetAce(acl, 0, &mut ace)?;
        anyhow::ensure!(!ace.is_null(), "IPC has no access rule");
        let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
        // ACCESS_ALLOWED_ACE_TYPE is 0. Reject other ACE layouts before reading SidStart.
        anyhow::ensure!(
            allowed.Header.AceType == 0
                && allowed.Header.AceSize as usize >= std::mem::size_of::<ACCESS_ALLOWED_ACE>(),
            "Unexpected IPC access rule"
        );
        EqualSid(
            owner,
            PSID((&allowed.SidStart as *const u32).cast_mut().cast()),
        )
        .context("IPC grants access to another principal")?;
    }
    Ok(())
}
