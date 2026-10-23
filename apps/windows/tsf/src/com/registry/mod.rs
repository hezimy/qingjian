//! 自注册：写 COM 的 InprocServer32，经 `ITfInputProcessorProfiles` / `ITfCategoryMgr` 把青简登记成键盘类文本服务。
//! 写的是 `HKEY_CLASSES_ROOT`，所以 regsvr32 要管理员。输入法图标文件在 [`icon`]。

mod icon;

use std::ffi::c_void;

use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIP_KEYBOARD, GUID_TFCAT_TIPCAP_COMLESS, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT, GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, GUID_TFCAT_TIPCAP_UIELEMENTENABLED, ITfCategoryMgr,
    ITfInputProcessorProfiles,
};
use windows::core::{GUID, PCWSTR, Result, s, w};
use windows_registry::CLASSES_ROOT;

use crate::com::{
    CLSID_QINGJIAN, CLSID_QINGJIAN_STR, GUID_PROFILE, GUID_PROFILE_STR, LANGID_ZH_CN,
    SERVICE_DESCRIPTION,
};

/// 除「键盘类 TIP」外还要声明沉浸式 / 系统托盘等能力，否则 Win10/11 的输入切换器会把它过滤掉（表现为「装上又消失」）。
const CATEGORIES: &[GUID] = &[
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
];

fn clsid_key() -> String {
    format!("CLSID\\{CLSID_QINGJIAN_STR}")
}

pub(crate) fn register() -> Result<()> {
    let module = crate::com::module_path()?;
    let base = CLASSES_ROOT.create(clsid_key())?;
    base.set_string("", SERVICE_DESCRIPTION)?;
    let inproc = base.create("InprocServer32")?;
    inproc.set_string("", module.to_string())?;
    inproc.set_string("ThreadingModel", "Apartment")?;
    register_profile()
}

/// 撤销 [`register`]，尽力而为。
pub(crate) fn unregister() -> Result<()> {
    let _ = unregister_profile();
    icon::uninstall();
    let _ = CLASSES_ROOT.remove_tree(clsid_key());
    Ok(())
}

fn register_profile() -> Result<()> {
    com_scope(|| {
        let profiles: ITfInputProcessorProfiles = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?
        };
        // AddLanguageProfile 按 null 扫描读字符串，不补 0 会多读相邻内存（曾显示成「青简C」）。
        let description = wide_z(SERVICE_DESCRIPTION);
        let icon = icon::install()
            .map(|path| wide_z(&path.to_string_lossy()))
            .unwrap_or_else(|| vec![0]);
        unsafe {
            profiles.Register(&CLSID_QINGJIAN)?;
            profiles.AddLanguageProfile(
                &CLSID_QINGJIAN,
                LANGID_ZH_CN,
                &GUID_PROFILE,
                &description,
                &icon,
                0,
            )?;
        }
        let category: ITfCategoryMgr =
            unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
        for catid in CATEGORIES {
            unsafe { category.RegisterCategory(&CLSID_QINGJIAN, catid, &CLSID_QINGJIAN)? };
        }
        install_layout_or_tip(false);
        Ok(())
    })
}

fn unregister_profile() -> Result<()> {
    com_scope(|| {
        install_layout_or_tip(true);
        if let Ok(category) = unsafe {
            CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
        } {
            for catid in CATEGORIES {
                let _ =
                    unsafe { category.UnregisterCategory(&CLSID_QINGJIAN, catid, &CLSID_QINGJIAN) };
            }
        }
        if let Ok(profiles) = unsafe {
            CoCreateInstance::<_, ITfInputProcessorProfiles>(
                &CLSID_TF_InputProcessorProfiles,
                None,
                CLSCTX_INPROC_SERVER,
            )
        } {
            unsafe {
                let _ =
                    profiles.RemoveLanguageProfile(&CLSID_QINGJIAN, LANGID_ZH_CN, &GUID_PROFILE);
                let _ = profiles.Unregister(&CLSID_QINGJIAN);
            }
        }
        Ok(())
    })
}

/// `InstallLayoutOrTip` 的「从列表移除」标志。
const ILOT_UNINSTALL: u32 = 0x0000_0001;

/// 用户输入法列表里的条目，格式见 `InstallLayoutOrTip` 文档。
fn layout_tip() -> String {
    format!("0x{LANGID_ZH_CN:04X}:{CLSID_QINGJIAN_STR}{GUID_PROFILE_STR}")
}

/// 把青简写进（或从）当前用户的输入法列表。
///
/// `AddLanguageProfile` 只把语言配置写进 CTF，**用户的输入法列表里没有它**——Win10/11 设置的
/// 「键盘」和「默认输入法」下拉都按 `HKCU\Control Panel\International\User Profile` 枚举，
/// 少了这一步就表现为「装好了，但设置里根本找不到青简」。
///
/// `input.dll` 没有导入库，按文档走 `LoadLibrary` + `GetProcAddress`。失败只记日志：语言配置
/// 已经注册成功，用户仍可从「文本服务和输入语言」手动添加。
fn install_layout_or_tip(uninstall: bool) {
    // 这条是当前用户的列表，与位数无关；32 位 regsvr32（走 WOW64 的 input.dll）不重复做。
    if !cfg!(target_pointer_width = "64") {
        return;
    }
    unsafe {
        let library = match LoadLibraryW(w!("input.dll")) {
            Ok(library) => library,
            Err(error) => {
                crate::com::log::log(&format!("加载 input.dll 失败，输入法列表未更新: {error}"));
                return;
            }
        };
        let Some(address) = GetProcAddress(library, s!("InstallLayoutOrTip")) else {
            crate::com::log::log("input.dll 没有 InstallLayoutOrTip，输入法列表未更新");
            let _ = FreeLibrary(library);
            return;
        };
        let install: unsafe extern "system" fn(PCWSTR, u32) -> i32 =
            std::mem::transmute::<*const c_void, _>(address as *const c_void);
        let tip = layout_tip();
        let wide: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if uninstall { ILOT_UNINSTALL } else { 0 };
        if install(PCWSTR(wide.as_ptr()), flags) == 0 {
            crate::com::log::log(&format!("InstallLayoutOrTip 失败: {tip} flags={flags}"));
        }
        let _ = FreeLibrary(library);
    }
}

/// regsvr32 一般已初始化过 COM；已是别的套间模式（`RPC_E_CHANGED_MODE`）就不配对反初始化。
fn com_scope<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = body();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

/// 以 0 结尾的宽字符串。
fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 格式错一位 `InstallLayoutOrTip` 就静默失败，表现为「装完设置里没有青简」。
    #[test]
    fn layout_tip_is_the_documented_shape() {
        assert_eq!(
            layout_tip(),
            "0x0804:{4FDCA82D-E923-49BF-9E75-BB906B93B8BB}{8119F8E0-CF81-423B-9189-C0D7374324B3}"
        );
    }
}
