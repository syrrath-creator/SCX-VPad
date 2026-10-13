//! Per-lease capability bootstrap. The normal user cannot read this registry
//! subtree. SYSTEM and elevated administrators can maintain it; the UMDF driver
//! SID only reads during device start. Filtered administrator membership grants
//! the ordinary UI no access, while recovery/uninstall can remove orphan state.
use super::SecurityDescriptor;
use scx_vpad_client::{
    win::{fail, wide},
    wire::{wipe, Capability},
};
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Cryptography::*, *},
    System::Registry::*,
};

const ROOT: &str = r"SOFTWARE\SCX\VirtualPad\Bootstrap";
const SDDL: &str = "O:SYG:SYD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;UD)";
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn result(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}
fn open_root() -> io::Result<Key> {
    let mut key = ptr::null_mut();
    result(unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(ROOT).as_ptr(),
            0,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            &mut key,
        )
    })?;
    Ok(Key(key))
}
pub(super) fn initialize() -> io::Result<()> {
    let sd = SecurityDescriptor::new(SDDL)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let mut key = ptr::null_mut();
    result(unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(ROOT).as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_VOLATILE,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            &sa,
            &mut key,
            ptr::null_mut(),
        )
    })?;
    let key = Key(key);
    // Existing state may remain after a service crash. Secure it before any new
    // secret is written, then remove only generated, strictly named lease keys.
    result(unsafe {
        RegSetKeySecurity(
            key.0,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            sd.0,
        )
    })?;
    for _ in 0..256 {
        let mut name = [0u16; 64];
        let mut length = name.len() as u32;
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                0,
                name.as_mut_ptr(),
                &mut length,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            return Ok(());
        }
        result(code)?;
        let id = String::from_utf16(&name[..length as usize])
            .map_err(|_| fail("invalid bootstrap key name"))?;
        if !valid_id(&id) {
            return Err(fail("unexpected child in protected bootstrap subtree"));
        }
        result(unsafe { RegDeleteTreeW(key.0, wide(&id).as_ptr()) })?;
    }
    Err(fail("too many orphan bootstrap keys"))
}
fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn random(bytes: &mut [u8]) -> io::Result<()> {
    if unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        return Err(fail("capability generation failed"));
    }
    Ok(())
}
pub(super) struct Bootstrap {
    pub(super) id: String,
    pub(super) capability: Capability,
    root: Key,
    key: Option<Key>,
}
impl Bootstrap {
    pub(super) fn create() -> io::Result<Self> {
        let mut public = [0u8; 16];
        random(&mut public)?;
        let id: String = public.iter().map(|b| format!("{b:02x}")).collect();
        let mut secret = [0u8; 32];
        if let Err(e) = random(&mut secret) {
            wipe(&mut secret);
            return Err(e);
        }
        let capability = Capability::new(secret);
        wipe(&mut secret);
        if capability.is_empty() {
            return Err(fail("invalid generated capability"));
        }
        let root = open_root()?;
        let sd = SecurityDescriptor::new(SDDL)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let mut key = ptr::null_mut();
        let mut disposition = 0;
        result(unsafe {
            RegCreateKeyExW(
                root.0,
                wide(&id).as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_VOLATILE,
                KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                &sa,
                &mut key,
                &mut disposition,
            )
        })?;
        let key = Key(key);
        if disposition != REG_CREATED_NEW_KEY {
            return Err(fail("bootstrap identifier collision"));
        }
        let entry = Self {
            id,
            capability,
            root,
            key: Some(key),
        };
        result(unsafe {
            RegSetValueExW(
                entry.key.as_ref().unwrap().0,
                wide("Capability").as_ptr(),
                0,
                REG_BINARY,
                entry.capability.bytes().as_ptr(),
                32,
            )
        })?;
        Ok(entry)
    }
}
impl Drop for Bootstrap {
    fn drop(&mut self) {
        // Device::drop has already completed SwDeviceClose before this field is
        // destroyed, so a live/restarting host never loses its bootstrap early.
        drop(self.key.take());
        unsafe {
            RegDeleteTreeW(self.root.0, wide(&self.id).as_ptr());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_key_names_cannot_escape_the_owned_subtree() {
        assert!(valid_id("0123456789abcdef0123456789abcdef"));
        for bad in [
            "",
            "..",
            "0123456789ABCDEF0123456789abcdef",
            "0123456789abcdef0123456789abcde/",
            "0123456789abcdef0123456789abcde\\",
        ] {
            assert!(!valid_id(bad));
        }
    }
}
