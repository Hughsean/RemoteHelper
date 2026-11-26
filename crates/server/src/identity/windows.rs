use anyhow::{Result, bail, ensure};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, Prefix};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::{DIRECTORY, FILENAME};

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut result: Vec<_> = value.encode_wide().collect();
    ensure!(!result.contains(&0), "身份路径不能包含空字符");
    result.push(0);
    Ok(result)
}

fn identity_directory(parent: &Path, create: bool) -> Result<std::path::PathBuf> {
    ensure!(
        matches!(parent.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            && parent.is_absolute(),
        "身份目录必须位于本地盘符路径下"
    );
    let path = parent.join(DIRECTORY);
    if create {
        match fs::create_dir(&path) {
            Ok(()) => (),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => (),
            Err(err) => return Err(err.into()),
        }
    }
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "身份目录必须是普通目录"
    );
    Ok(path)
}

pub fn read<T>(parent: &Path, read: impl FnOnce(File) -> Result<T>) -> Result<T> {
    let path = identity_directory(parent, false)?.join(FILENAME);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "身份文件必须是普通文件"
    );
    read(file)
}

pub fn publish(
    parent: &Path,
    reader: Option<&str>,
    write: impl FnOnce(File) -> Result<()>,
) -> Result<()> {
    if reader.is_some() {
        bail!("Windows 初始化不再接受服务账户标识；身份文件使用部署目录的继承权限");
    }
    let path = identity_directory(parent, true)?;
    let temp = path.join(format!(".identity-{:032x}.tmp", rand::random::<u128>()));
    let source = wide(temp.as_os_str())?;
    let target = wide(path.join(FILENAME).as_os_str())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        write(file)?;
        // 同目录原子发布，不覆盖已存在的身份文件。
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_directory_can_initialize_reload_and_preserve_identity() -> Result<()> {
        let path = std::env::temp_dir().join(format!(
            "remotehelper-identity-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path)?;
        let config = path.join("config.toml");
        let result = (|| -> Result<()> {
            let first = super::super::initialize(&config, None)?.fingerprint();
            assert!(super::super::initialize(&config, None).is_err());
            assert_eq!(super::super::load(&config)?.fingerprint(), first);
            Ok(())
        })();
        fs::remove_dir_all(path)?;
        result
    }
}
