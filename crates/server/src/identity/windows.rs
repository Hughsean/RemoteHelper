use anyhow::{Context, Result, bail, ensure};
use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;

use super::{DIRECTORY, FILENAME};

const ADMIN: &str = "S-1-5-32-544";
const SYSTEM: &str = "S-1-5-18";
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
const DIR_FLAGS: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
const FILE_FLAGS: u32 = FILE_FLAG_OPEN_REPARSE_POINT | FILE_ATTRIBUTE_NORMAL;
const SHARE_READ_WRITE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

fn service_sid(value: &str) -> bool {
    matches!(value, "S-1-5-19" | "S-1-5-20")
        || value.strip_prefix("S-1-5-80-").is_some_and(|suffix| {
            let mut parts = suffix.split('-');
            (0..5).all(|_| {
                parts
                    .next()
                    .is_some_and(|part| part.parse::<u32>().is_ok_and(|n| n.to_string() == part))
            }) && parts.next().is_none()
        })
}

fn trusted(sid: &str) -> bool {
    matches!(sid, ADMIN | SYSTEM | TRUSTED_INSTALLER)
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn wide(s: &OsStr) -> Result<Vec<u16>> {
    let mut w: Vec<_> = s.encode_wide().collect();
    ensure!(!w.contains(&0), "identity path contains NUL");
    w.push(0);
    Ok(w)
}

fn bool_result(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn sid_text(sid: PSID) -> Result<String> {
    ensure!(
        !sid.is_null() && unsafe { IsValidSid(sid) } != 0,
        "invalid ACL SID"
    );
    let mut value = null_mut();
    bool_result(unsafe { ConvertSidToStringSidW(sid, &mut value) })?;
    let _value = LocalAllocation(value.cast());
    let len = (0..256)
        .find(|&i| unsafe { *value.add(i) == 0 })
        .context("invalid ACL SID text")?;
    Ok(String::from_utf16(unsafe {
        std::slice::from_raw_parts(value, len)
    })?)
}

fn admin_token() -> Result<()> {
    let mut sid = null_mut();
    bool_result(unsafe { ConvertStringSidToSidW(wide(OsStr::new(ADMIN))?.as_ptr(), &mut sid) })?;
    let _sid = LocalAllocation(sid.cast());
    let mut member = 0;
    bool_result(unsafe { CheckTokenMembership(null_mut(), sid, &mut member) })?;
    ensure!(
        member != 0,
        "identity initialization requires an elevated Windows administrator"
    );
    Ok(())
}

struct SecurityAttributes(LocalAllocation);
impl SecurityAttributes {
    fn new(reader: Option<&str>, directory: bool) -> Result<Self> {
        let access = if directory { "0x1200a9" } else { "0x120089" };
        let mut sddl = String::from("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)");
        if let Some(sid) = reader {
            sddl.push_str(&format!("(A;;{access};;;{sid})"));
        }
        let mut sd = null_mut();
        bool_result(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(OsStr::new(&sddl))?.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        })?;
        Ok(Self(LocalAllocation(sd)))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: 0,
        }
    }
}

fn open(
    path: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    flags: u32,
    attrs: *const SECURITY_ATTRIBUTES,
) -> Result<File> {
    let handle = unsafe {
        CreateFileW(
            wide(path.as_os_str())?.as_ptr(),
            access,
            share,
            attrs,
            disposition,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn object_type(file: &File, directory: bool) -> Result<()> {
    let handle = file.as_raw_handle();
    ensure!(
        unsafe { GetFileType(handle) } == FILE_TYPE_DISK,
        "identity path is not a local disk file"
    );
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    bool_result(unsafe { GetFileInformationByHandle(handle, &mut info) })?;
    ensure!(
        info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "identity path contains a reparse point"
    );
    ensure!(
        (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) == directory,
        "unexpected identity object type"
    );
    ensure!(
        directory || info.nNumberOfLinks == 1,
        "identity file has multiple hard links"
    );
    Ok(())
}

/// 对已打开的句柄检查属主、DACL 及 ACE；本地服务只能读，不能继承额外权限。
fn verify_acl(file: &File, strict: bool, directory: bool) -> Result<Option<String>> {
    let mut owner = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut sd = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    let _sd = LocalAllocation(sd.cast());
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32).into());
    }
    ensure!(
        !dacl.is_null() && unsafe { IsValidAcl(dacl) } != 0,
        "identity path has no valid DACL"
    );
    let owner = sid_text(owner)?;
    ensure!(
        if strict {
            matches!(owner.as_str(), ADMIN | SYSTEM)
        } else {
            trusted(&owner)
        },
        "identity path is not owned by an administrator or system principal"
    );
    if strict {
        let mut control = 0;
        let mut revision = 0;
        bool_result(unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) })?;
        ensure!(
            control & SE_DACL_PROTECTED != 0,
            "identity ACL permits unsafe inheritance"
        );
    }
    let mut admin = false;
    let mut system = false;
    let mut reader = None;
    for i in 0..unsafe { (*dacl).AceCount } {
        let mut ace = null_mut();
        bool_result(unsafe { GetAce(dacl, i.into(), &mut ace) })?;
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if !strict && (u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 || header.AceType == 1) {
            continue;
        }
        // 0 是 ACCESS_ALLOWED_ACE_TYPE；未知可授权 ACE 一律拒绝。
        ensure!(
            header.AceType == 0
                && usize::from(header.AceSize) >= size_of::<ACCESS_ALLOWED_ACE>() + 4,
            "identity path has an unsupported ACE"
        );
        let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid_start: PSID = (&raw const ace.SidStart).cast_mut().cast();
        let sid_size = 8 + 4 * usize::from(unsafe { *sid_start.cast::<u8>().add(1) });
        ensure!(
            sid_size <= usize::from(header.AceSize) - 8,
            "invalid identity ACE SID length"
        );
        let sid = sid_text(sid_start)?;
        if strict {
            ensure!(header.AceFlags == 0, "identity ACE must not inherit");
            match sid.as_str() {
                ADMIN if !admin && ace.Mask == FILE_ALL_ACCESS => admin = true,
                SYSTEM if !system && ace.Mask == FILE_ALL_ACCESS => system = true,
                sid if service_sid(sid) && reader.is_none() => {
                    let allowed = FILE_GENERIC_READ | if directory { FILE_TRAVERSE } else { 0 };
                    ensure!(
                        ace.Mask == allowed,
                        "identity service reader must only have read access"
                    );
                    reader = Some(sid.to_string());
                }
                _ => bail!("unexpected principal or access in identity ACL"),
            }
        } else if !trusted(&sid) {
            // 祖先允许大家读取或创建其他对象，但不得删除或更改受保护路径。
            ensure!(
                ace.Mask
                    & (WRITE_DAC
                        | WRITE_OWNER
                        | FILE_DELETE_CHILD
                        | FILE_WRITE_ATTRIBUTES
                        | FILE_WRITE_DATA
                        | DELETE
                        | GENERIC_ALL
                        | GENERIC_WRITE)
                    == 0,
                "untrusted principal could alter the identity directory ancestry"
            );
        }
    }
    ensure!(
        !strict || (admin && system),
        "identity ACL lacks administrator or SYSTEM control"
    );
    Ok(reader)
}

struct CheckedParent {
    path: PathBuf,
    // 保持所有目录句柄打开且不共享删除，防止路径在后续访问中被替换。
    _locked_ancestors: Vec<File>,
}

fn checked_parent(path: &Path) -> Result<CheckedParent> {
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        bail!("expected absolute disk identity path");
    };
    ensure!(
        matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)),
        "identity is supported only on a local fixed disk"
    );
    ensure!(
        matches!(parts.next(), Some(Component::RootDir)),
        "expected absolute disk identity path"
    );
    let mut path = PathBuf::from(prefix.as_os_str());
    path.push("\\");
    // DRIVE_FIXED = 3；拒绝 SMB、映射盘及可移动文件系统。
    ensure!(
        unsafe { GetDriveTypeW(wide(path.as_os_str())?.as_ptr()) } == 3,
        "identity is supported only on a local fixed disk"
    );
    let mut locked = Vec::new();
    let root = open(
        &path,
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        SHARE_READ_WRITE,
        OPEN_EXISTING,
        DIR_FLAGS,
        null(),
    )?;
    object_type(&root, true)?;
    verify_acl(&root, false, true)?;
    locked.push(root);
    for part in parts {
        let Component::Normal(base) = part else {
            bail!("invalid identity directory component");
        };
        let base_utf16 = wide(base)?;
        ensure!(
            !base_utf16.contains(&(b':' as u16))
                && !matches!(base_utf16.get(base_utf16.len() - 2), Some(32 | 46)),
            "ambiguous identity directory component"
        );
        path.push(base);
        let dir = open(
            &path,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            SHARE_READ_WRITE,
            OPEN_EXISTING,
            DIR_FLAGS,
            null(),
        )?;
        object_type(&dir, true)?;
        verify_acl(&dir, false, true)?;
        locked.push(dir);
    }
    Ok(CheckedParent {
        path,
        _locked_ancestors: locked,
    })
}

pub fn read<T>(parent: &Path, read: impl FnOnce(File) -> Result<T>) -> Result<T> {
    let parent = checked_parent(parent)?;
    let path = parent.path.join(DIRECTORY);
    let directory = open(
        &path,
        FILE_GENERIC_READ,
        SHARE_READ_WRITE,
        OPEN_EXISTING,
        DIR_FLAGS,
        null(),
    )?;
    object_type(&directory, true)?;
    let reader = verify_acl(&directory, true, true)?;
    let file = open(
        &path.join(FILENAME),
        FILE_GENERIC_READ,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        FILE_FLAGS,
        null(),
    )?;
    object_type(&file, false)?;
    ensure!(
        verify_acl(&file, true, false)? == reader,
        "identity reader SID differs from its directory"
    );
    read(file)
}

pub fn publish(
    parent: &Path,
    reader: Option<&str>,
    write: impl FnOnce(File) -> Result<()>,
) -> Result<()> {
    admin_token()?;
    let reader = reader.filter(|sid| *sid != SYSTEM);
    ensure!(
        reader.is_none_or(service_sid),
        "reader must be LocalService, NetworkService, or a per-service SID"
    );
    let parent = checked_parent(parent)?;
    let path = parent.path.join(DIRECTORY);
    let security = SecurityAttributes::new(reader, true)?;
    if let Err(error) = bool_result(unsafe {
        CreateDirectoryW(wide(path.as_os_str())?.as_ptr(), &security.attributes())
    }) {
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    let directory = open(
        &path,
        FILE_GENERIC_READ,
        SHARE_READ_WRITE,
        OPEN_EXISTING,
        DIR_FLAGS,
        null(),
    )?;
    object_type(&directory, true)?;
    ensure!(
        verify_acl(&directory, true, true)?.as_deref() == reader,
        "existing identity directory reader SID does not match"
    );
    let temp = path.join(format!(".identity-{:032x}.tmp", rand::random::<u128>()));
    let target = path.join(FILENAME);
    let source = wide(temp.as_os_str())?;
    let target = wide(target.as_os_str())?;
    let security = SecurityAttributes::new(reader, false)?;
    let file = open(
        &temp,
        FILE_GENERIC_WRITE | READ_CONTROL,
        0,
        CREATE_NEW,
        FILE_FLAGS,
        &security.attributes(),
    )?;
    let result: Result<()> = (|| {
        object_type(&file, false)?;
        ensure!(
            verify_acl(&file, true, false)?.as_deref() == reader,
            "identity reader SID mismatch"
        );
        write(file)?;
        // 不设置 REPLACE_EXISTING 或 COPY_ALLOWED：同目录发布，绝不覆盖。
        bool_result(unsafe {
            MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH)
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_sid_cannot_inject_acl_entries_or_allow_general_groups() {
        assert!(service_sid("S-1-5-19"));
        assert!(service_sid("S-1-5-20"));
        assert!(service_sid("S-1-5-80-1-2-3-4-5"));
        assert!(!service_sid("S-1-5-80-0"));
        assert!(!service_sid("S-1-5-80-1-2-3-4-5)(A;;FA;;;WD)"));
        assert!(!service_sid("S-1-5-32-545"));
        assert!(SecurityAttributes::new(Some("S-1-5-19"), true).is_ok());
        assert!(SecurityAttributes::new(None, false).is_ok());
    }

    #[test]
    fn rejects_unsafe_platform_paths_before_using_identity() {
        assert!(checked_parent(Path::new(r"\\localhost\C$\RemoteHelper")).is_err());
        assert!(checked_parent(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }
}
