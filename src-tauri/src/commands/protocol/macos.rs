//! macOS file and URL associations through LaunchServices.
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};

/// Returns the bundle identifier of the app registered as the default
/// handler for the given URL scheme, or `None` if no handler is set.
pub fn get_default_handler_bundle_id(protocol: &str) -> Option<String> {
    if protocol == ".torrent" {
        use core_foundation::{base::TCFType, string::CFString};
        let content_type = torrent_content_type()?;
        // SAFETY: LaunchServices returns an owned CFString or null.
        let handler = unsafe {
            LSCopyDefaultRoleHandlerForContentType(content_type.as_concrete_TypeRef(), u32::MAX)
        };
        return (!handler.is_null())
            .then(|| unsafe { CFString::wrap_under_create_rule(handler) }.to_string());
    }
    let workspace = NSWorkspace::sharedWorkspace();
    let url_str = format!("{protocol}://test");
    let ns_url_str = NSString::from_str(&url_str);
    let test_url = NSURL::URLWithString(&ns_url_str)?;
    let handler_url = workspace.URLForApplicationToOpenURL(&test_url)?;
    let handler_bundle = NSBundle::bundleWithURL(&handler_url)?;
    let bundle_id = handler_bundle.bundleIdentifier()?;
    Some(bundle_id.to_string())
}

/// Registers this application as the default handler for the given URL
/// scheme using `LSSetDefaultHandlerForURLScheme`.
pub fn set_as_default_handler(protocol: &str, bundle_id: &str) -> Result<(), String> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    if protocol == ".torrent" {
        let content_type =
            torrent_content_type().ok_or("Cannot resolve the torrent content type")?;
        let handler = CFString::new(bundle_id);
        // SAFETY: Both CFStrings remain alive for the synchronous call.
        let status = unsafe {
            LSSetDefaultRoleHandlerForContentType(
                content_type.as_concrete_TypeRef(),
                u32::MAX,
                handler.as_concrete_TypeRef(),
            )
        };
        return match status {
            0 => Ok(()),
            -128 => Err("cancelled".into()),
            _ => Err(format!("LaunchServices returned {status}")),
        };
    }
    let scheme = CFString::new(protocol);
    let handler = CFString::new(bundle_id);

    let status = unsafe {
        core_foundation::base::OSStatus::from(LSSetDefaultHandlerForURLScheme(
            scheme.as_concrete_TypeRef(),
            handler.as_concrete_TypeRef(),
        ))
    };
    if status == 0 {
        Ok(())
    } else if status == -128 {
        // LaunchServices userCanceledErr is a normal dismissal.
        Err("cancelled".into())
    } else {
        Err(format!("LSSetDefaultHandlerForURLScheme returned {status}"))
    }
}

fn torrent_content_type() -> Option<core_foundation::string::CFString> {
    use core_foundation::{base::TCFType, string::CFString};
    let tag = CFString::new("public.filename-extension");
    let extension = CFString::new("torrent");
    // SAFETY: The returned type follows the Core Foundation Create rule.
    let result = unsafe {
        UTTypeCreatePreferredIdentifierForTag(
            tag.as_concrete_TypeRef(),
            extension.as_concrete_TypeRef(),
            std::ptr::null(),
        )
    };
    (!result.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(result) })
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn UTTypeCreatePreferredIdentifierForTag(
        tag: core_foundation::string::CFStringRef,
        value: core_foundation::string::CFStringRef,
        conforms: core_foundation::string::CFStringRef,
    ) -> core_foundation::string::CFStringRef;
    fn LSCopyDefaultRoleHandlerForContentType(
        content_type: core_foundation::string::CFStringRef,
        roles: u32,
    ) -> core_foundation::string::CFStringRef;
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: core_foundation::string::CFStringRef,
        roles: u32,
        handler: core_foundation::string::CFStringRef,
    ) -> i32;
    fn LSSetDefaultHandlerForURLScheme(
        scheme: core_foundation::string::CFStringRef,
        handler: core_foundation::string::CFStringRef,
    ) -> i32;
}
