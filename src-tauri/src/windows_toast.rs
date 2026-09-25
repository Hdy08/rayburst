//! Desktop toast COM activation, independent of the WebView and protocol launcher.

use std::{ffi::c_void, marker::PhantomData, path::Path, rc::Rc, sync::Arc};

use windows::{
    core::{implement, Error, IUnknown, Interface, Result, GUID, PCWSTR},
    Win32::{
        Foundation::{BOOL, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_POINTER},
        System::Com::{
            CoInitializeEx, CoRegisterClassObject, CoRevokeClassObject, CoUninitialize,
            IClassFactory, IClassFactory_Impl, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
            REGCLS_MULTIPLEUSE,
        },
        UI::Notifications::{
            INotificationActivationCallback, INotificationActivationCallback_Impl,
            NOTIFICATION_USER_INPUT_DATA,
        },
    },
};
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

pub const LAUNCH_ARG: &str = "--notification-activation";
const MAX_ACTIVATION_CHARS: usize = 32_768;
const INSTALLED_APP_ID: &str = env!("DESKTOP_APP_ID");
const INSTALLED_CLSID: GUID = GUID::from_u128(0x22fc9aa3_1a56_47ff_a6a5_62d3e230a135);

#[derive(Clone)]
pub struct Identity {
    pub app_id: String,
    pub clsid: GUID,
}

impl Identity {
    pub fn for_app() -> Self {
        if cfg!(debug_assertions) {
            Self {
                app_id: format!("{INSTALLED_APP_ID}.debug"),
                clsid: GUID::from_u128(0x4402830b_5f6f_4fb4_bbe6_153e52b95455),
            }
        } else {
            Self {
                app_id: INSTALLED_APP_ID.into(),
                clsid: INSTALLED_CLSID,
            }
        }
    }

    fn clsid_string(&self) -> String {
        format!("{{{:?}}}", self.clsid)
    }

    fn server_key(&self) -> String {
        format!(
            r"Software\Classes\CLSID\{}\LocalServer32",
            self.clsid_string()
        )
    }

    fn app_key(&self) -> String {
        format!(r"Software\Classes\AppUserModelId\{}", self.app_id)
    }

    fn register(&self, exe: &Path) -> Result<()> {
        let command = server_command(exe)?;
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let (server, _) = root
            .create_subkey(self.server_key())
            .map_err(registry_error)?;
        server.set_value("", &command).map_err(registry_error)?;
        let (app, _) = root.create_subkey(self.app_key()).map_err(registry_error)?;
        app.set_value("DisplayName", &"Rayburst")
            .map_err(registry_error)?;
        app.set_value("CustomActivator", &self.clsid_string())
            .map_err(registry_error)?;
        Ok(())
    }

    /// Only remove registration still pointing at this executable. Other installs
    /// and unrelated AppUserModelId properties must remain untouched.
    #[cfg(test)]
    pub fn unregister(&self, exe: &Path) -> Result<()> {
        use winreg::enums::{KEY_READ, KEY_WRITE};

        let root = RegKey::predef(HKEY_CURRENT_USER);
        let expected = server_command(exe)?;
        let registered: Option<String> = root
            .open_subkey(self.server_key())
            .ok()
            .and_then(|key| key.get_value("").ok());
        if registered.as_deref() != Some(&expected) {
            return Ok(());
        }
        if let Ok(app) = root.open_subkey_with_flags(self.app_key(), KEY_READ | KEY_WRITE) {
            let activator: Option<String> = app.get_value("CustomActivator").ok();
            if activator.as_deref() == Some(&self.clsid_string()) {
                app.delete_value("CustomActivator")
                    .map_err(registry_error)?;
            }
        }
        root.delete_subkey(self.server_key())
            .map_err(registry_error)?;
        Ok(())
    }
}

fn server_command(exe: &Path) -> Result<String> {
    if !exe.is_absolute() {
        return Err(Error::new(
            E_INVALIDARG,
            "Notification executable must be absolute",
        ));
    }
    let Some(exe) = exe.to_str().filter(|value| !value.contains(['"', '\0'])) else {
        return Err(Error::new(
            E_INVALIDARG,
            "Invalid notification executable path",
        ));
    };
    Ok(format!("\"{exe}\" {LAUNCH_ARG}"))
}

fn registry_error(error: std::io::Error) -> Error {
    Error::new(E_FAIL, format!("Notification registration: {error}"))
}

type ActivationHandler = Arc<dyn Fn(String) + Send + Sync>;

#[implement(INotificationActivationCallback)]
struct Activator {
    app_id: String,
    handler: ActivationHandler,
}

impl INotificationActivationCallback_Impl for Activator {
    fn Activate(
        &self,
        app_id: &PCWSTR,
        arguments: &PCWSTR,
        _data: *const NOTIFICATION_USER_INPUT_DATA,
        count: u32,
    ) -> Result<()> {
        // COM marshals these as null-terminated strings. Bound the scan and
        // reject input controls: Motrix notifications only carry action URLs.
        let app_id = unsafe { bounded_string(*app_id, 256)? };
        let arguments = unsafe { bounded_string(*arguments, MAX_ACTIVATION_CHARS)? };
        if app_id != self.app_id || count != 0 {
            return Err(Error::new(E_INVALIDARG, "Invalid notification activation"));
        }
        (self.handler)(arguments);
        Ok(())
    }
}

unsafe fn bounded_string(value: PCWSTR, limit: usize) -> Result<String> {
    if value.is_null() {
        return Err(Error::from_hresult(E_POINTER));
    }
    for length in 0..=limit {
        if *value.0.add(length) == 0 {
            return String::from_utf16(std::slice::from_raw_parts(value.0, length))
                .map_err(|_| Error::from_hresult(E_INVALIDARG));
        }
    }
    Err(Error::new(
        E_INVALIDARG,
        "Notification argument exceeds limit",
    ))
}

#[implement(IClassFactory)]
struct Factory {
    app_id: String,
    handler: ActivationHandler,
}

impl IClassFactory_Impl for Factory {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if object.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        unsafe { *object = std::ptr::null_mut() };
        if iid.is_null() {
            return Err(Error::from_hresult(E_POINTER));
        }
        if outer.is_some() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let activator: INotificationActivationCallback = Activator {
            app_id: self.app_id.clone(),
            handler: self.handler.clone(),
        }
        .into();
        unsafe { (activator.vtable().base__.QueryInterface)(activator.as_raw(), iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// Owned by the registering STA and revoked there on shutdown. No extra thread
/// or per-toast callback needs to survive after a notification is submitted.
pub struct Registration {
    cookie: u32,
    _apartment: Apartment,
}

impl Registration {
    pub fn new(identity: &Identity, handler: ActivationHandler) -> Result<Self> {
        let apartment = Apartment::new()?;
        let factory: IClassFactory = Factory {
            app_id: identity.app_id.clone(),
            handler,
        }
        .into();
        let cookie = unsafe {
            CoRegisterClassObject(
                &identity.clsid,
                &factory,
                CLSCTX_LOCAL_SERVER,
                REGCLS_MULTIPLEUSE,
            )?
        };
        let registration = Self {
            cookie,
            _apartment: apartment,
        };
        let exe = std::env::current_exe().map_err(registry_error)?;
        identity.register(&exe)?;
        Ok(registration)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Err(error) = unsafe { CoRevokeClassObject(self.cookie) } {
            log::warn!("notification:com-revoke-failed error={error}");
        }
    }
}

pub struct Apartment(PhantomData<Rc<()>>);

impl Apartment {
    pub fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self(PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod probe;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstall_hook_matches_installed_notification_identity() {
        let hook = include_str!("../nsis/hooks.nsh").to_uppercase();
        assert!(hook.contains(&format!("{{{INSTALLED_CLSID:?}}}")));
        assert!(hook.contains(&INSTALLED_APP_ID.to_uppercase()));
        assert!(hook.contains("NSIS_HOOK_POSTUNINSTALL"));
        assert!(hook.contains("$UPDATEMODE != 1"));
    }

    #[test]
    fn quotes_executable_and_rejects_invalid_commands() {
        assert_eq!(
            server_command(Path::new(r"C:\Program Files\Rayburst\rayburst.exe")).unwrap(),
            r#""C:\Program Files\Rayburst\rayburst.exe" --notification-activation"#
        );
        assert!(server_command(Path::new("relative.exe")).is_err());
        assert!(server_command(Path::new("C:\\bad\"path.exe")).is_err());
    }

    #[test]
    fn bounds_and_validates_marshaled_arguments() {
        let valid = [65_u16, 0];
        assert_eq!(
            unsafe { bounded_string(PCWSTR(valid.as_ptr()), 1) }.unwrap(),
            "A"
        );
        assert!(unsafe { bounded_string(PCWSTR(valid.as_ptr()), 0) }.is_err());
        assert!(unsafe { bounded_string(PCWSTR::null(), 1) }.is_err());
        let invalid = [0xd800, 0];
        assert!(unsafe { bounded_string(PCWSTR(invalid.as_ptr()), 1) }.is_err());
    }

    #[test]
    fn activator_rejects_wrong_identity_and_input_controls() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let count_calls = calls.clone();
        let callback = Activator {
            app_id: "test".into(),
            handler: Arc::new(move |_| {
                count_calls.fetch_add(1, Ordering::SeqCst);
            }),
        };
        assert!(callback
            .Activate(
                &windows::core::w!("other"),
                &windows::core::w!("rayburst://activate"),
                std::ptr::null(),
                0
            )
            .is_err());
        assert!(callback
            .Activate(
                &windows::core::w!("test"),
                &windows::core::w!("rayburst://activate"),
                std::ptr::null(),
                1
            )
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(callback
            .Activate(
                &windows::core::w!("test"),
                &windows::core::w!("rayburst://activate"),
                std::ptr::null(),
                0
            )
            .is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn factory_clears_output_when_interface_id_is_invalid() {
        let factory = Factory {
            app_id: "test".into(),
            handler: Arc::new(|_| {}),
        };
        let mut output = std::ptr::dangling_mut::<c_void>();
        assert!(factory
            .CreateInstance(None, std::ptr::null(), &mut output)
            .is_err());
        assert!(output.is_null());
    }
}
