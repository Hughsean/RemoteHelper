use anyhow::{Context, Result, bail, ensure};
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use super::{DIRECTORY, FILENAME};

fn component(name: &str) -> CString {
    CString::new(name).expect("常量文件名无空字节")
}

fn open_at(parent: &File, name: &CStr, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_dir(parent: &File, name: &CStr) -> io::Result<File> {
    open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
}

fn check_parent(directory: &File) -> Result<()> {
    let metadata = directory.metadata()?;
    ensure!(
        metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "identity path must have root-owned, non-group/world-writable ancestors"
    );
    Ok(())
}

fn parent_dir(path: &Path) -> Result<File> {
    let mut directory = File::open("/")?;
    check_parent(&directory)?;
    for part in path.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(name) => {
                directory = open_dir(&directory, &CString::new(name.as_bytes())?)?;
                check_parent(&directory)?;
            }
            _ => bail!("invalid identity directory path"),
        }
    }
    Ok(directory)
}

fn ensure_directory(directory: &File) -> Result<()> {
    let meta = directory.metadata()?;
    ensure!(
        meta.is_dir() && meta.uid() == 0 && matches!(meta.mode() & 0o7777, 0o700 | 0o750),
        "unsafe identity directory owner or permissions"
    );
    Ok(())
}

fn ensure_file(directory: &File, file: &File) -> Result<()> {
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == 0
            && matches!(meta.mode() & 0o7777, 0o600 | 0o640),
        "unsafe identity file type, owner, permissions or link count"
    );
    if meta.mode() & 0o040 != 0 {
        let dir = directory.metadata()?;
        ensure!(
            dir.mode() & 0o050 == 0o050 && dir.gid() == meta.gid(),
            "identity reader group mismatch"
        );
    }
    Ok(())
}

fn set_access(file: &File, group: Option<libc::gid_t>, mode: libc::mode_t) -> Result<()> {
    let fd = file.as_raw_fd();
    if let Some(group) = group {
        ensure!(group != !0, "invalid service reader GID");
        if unsafe { libc::fchown(fd, 0, group) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    if unsafe { libc::fchmod(fd, mode) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn read<T>(parent: &Path, read: impl FnOnce(File) -> Result<T>) -> Result<T> {
    let parent = parent_dir(parent)?;
    let directory = open_dir(&parent, &component(DIRECTORY))?;
    ensure_directory(&directory)?;
    let file = open_at(
        &directory,
        &component(FILENAME),
        libc::O_RDONLY | libc::O_NONBLOCK,
        0,
    )?;
    ensure_file(&directory, &file)?;
    read(file)
}

pub fn publish(
    parent: &Path,
    reader: Option<&str>,
    write: impl FnOnce(File) -> Result<()>,
) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "identity initialization must run as root"
    );
    let group = reader
        .map(str::parse::<libc::gid_t>)
        .transpose()
        .context("expected numeric service reader GID")?;
    ensure!(group != Some(!0), "invalid service reader GID");
    let parent = parent_dir(parent)?;
    let dir_name = component(DIRECTORY);
    let created = unsafe { libc::mkdirat(parent.as_raw_fd(), dir_name.as_ptr(), 0o700) } == 0;
    if !created {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    let directory = open_dir(&parent, &dir_name)?;
    if created {
        set_access(
            &directory,
            group,
            if group.is_some() { 0o750 } else { 0o700 },
        )?;
        parent.sync_all()?;
    }
    ensure_directory(&directory)?;
    let metadata = directory.metadata()?;
    ensure!(
        group.map_or(metadata.mode() & 0o077 == 0, |gid| metadata.gid() == gid
            && metadata.mode() & 0o050 == 0o050),
        "existing identity directory does not match the requested reader group"
    );

    let temp = component(&format!(".identity-{:032x}.tmp", rand::random::<u128>()));
    let name = component(FILENAME);
    let file = open_at(
        &directory,
        &temp,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    let fd = directory.as_raw_fd();
    let result = (|| {
        set_access(&file, group, if group.is_some() { 0o640 } else { 0o600 })?;
        ensure_file(&directory, &file)?;
        write(file)?;
        // 同目录原子发布，绝不覆盖现有目标；链接仅用作临时发布原语。
        if unsafe { libc::linkat(fd, temp.as_ptr(), fd, name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error())
                .context("identity already exists or could not be installed");
        }
        Ok(())
    })();
    let unlinked = unsafe { libc::unlinkat(fd, temp.as_ptr(), 0) } == 0;
    result?;
    ensure!(
        unlinked,
        "publication requires administrator cleanup of the temporary identity link"
    );
    directory.sync_all()?;
    Ok(())
}
