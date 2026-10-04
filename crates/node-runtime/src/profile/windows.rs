//! Windows has no Unix permission modes. Create the profile with an explicit,
//! inheritable user DACL, and inspect existing objects without changing them.
use std::{
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::{GENERIC_ALL, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid,
        GetSecurityDescriptorControl, GetTokenInformation, INHERIT_ONLY_ACE, InitializeAcl,
        InitializeSecurityDescriptor, IsValidAcl, IsValidSecurityDescriptor, IsValidSid,
        IsWellKnownSid, NO_PROPAGATE_INHERIT_ACE, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION,
        PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
        SetSecurityDescriptorControl, SetSecurityDescriptorDacl, SetSecurityDescriptorOwner,
        TOKEN_QUERY, TOKEN_USER, TokenUser, WinLocalSystemSid,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, GetFileInformationByHandle, READ_CONTROL,
    },
    System::{
        SystemServices::{ACCESS_ALLOWED_ACE_TYPE, SECURITY_DESCRIPTOR_REVISION},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

use super::invalid;
use crate::Error;

pub(super) fn directory(path: &Path) -> Result<(), Error> {
    match path.symlink_metadata() {
        Ok(_) => return validate(path, true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "data directory has no parent"))?;
    std::fs::create_dir_all(parent)?;
    // The canonical parent supplies the verbatim Windows prefix for long paths.
    let creation_path = parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| invalid(path, "data directory has no name"))?,
    );
    let mut security = Private::new()?;
    let attributes = security.attributes();
    let name = wide(&creation_path)?;
    // SAFETY: the name and complete descriptor remain alive throughout creation.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    validate(path, true)
}

pub(crate) fn validate_existing_file(path: &Path) -> Result<(), Error> {
    match path.symlink_metadata() {
        Ok(_) => validate(path, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// SQLite creates sidecars with inherited security, so its parent must grant
/// access only to this user and SYSTEM before the connection can write anything.
pub(crate) fn prepare_database(path: &Path) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "database directory is missing"))?;
    validate(parent, true)?;
    validate_database(path)?;
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => drop(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    validate(path, false)
}

pub(crate) fn validate_database(path: &Path) -> Result<(), Error> {
    validate_existing_file(path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        validate_existing_file(Path::new(&name))?;
    }
    Ok(())
}

fn validate(path: &Path, directory: bool) -> Result<(), Error> {
    let file = OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file handle and writable output struct are valid.
    check(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) })?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid(path, "profile storage must not be a reparse point"));
    }
    if (information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory {
        return Err(invalid(path, "profile storage has an unexpected file type"));
    }
    if !directory && information.nNumberOfLinks != 1 {
        return Err(invalid(path, "profile storage must not be hard-linked"));
    }
    let owner = User::current()?;
    let security = Security::read(&file)?;
    security.validate(path, owner.sid(), directory)
}

struct Security {
    allocation: Allocation,
    owner: PSID,
    dacl: *mut ACL,
}

impl Security {
    fn read(file: &File) -> io::Result<Self> {
        let mut descriptor = null_mut();
        let mut owner = null_mut();
        let mut dacl = null_mut();
        // SAFETY: the open file is live and all outputs are writable pointers.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        let allocation = Allocation(descriptor);
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(Self {
            allocation,
            owner,
            dacl,
        })
    }

    fn validate(&self, path: &Path, user: PSID, directory: bool) -> Result<(), Error> {
        // SAFETY: GetSecurityInfo owns the descriptor, owner and ACL until drop.
        if self.owner.is_null() || unsafe { EqualSid(self.owner, user) } == 0 {
            return Err(invalid(
                path,
                "profile storage must belong to the current user",
            ));
        }
        if self.dacl.is_null()
            || unsafe { IsValidSecurityDescriptor(self.allocation.0) } == 0
            || unsafe { IsValidAcl(self.dacl) } == 0
        {
            return Err(invalid(
                path,
                "profile storage requires a private access list",
            ));
        }
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: the descriptor is valid, and both output values are writable.
        check(unsafe {
            GetSecurityDescriptorControl(self.allocation.0, &mut control, &mut revision)
        })?;
        if directory && control & SE_DACL_PROTECTED == 0 {
            return Err(invalid(
                path,
                "data directory must exclude inherited access",
            ));
        }
        let mut user_access = false;
        // SAFETY: IsValidAcl has checked this ACL's header and contained entries.
        for index in 0..unsafe { (*self.dacl).AceCount } as u32 {
            let mut entry = null_mut();
            check(unsafe { GetAce(self.dacl, index, &mut entry) })?;
            // GetAce can also return other ACE layouts. Read only its common
            // header until the type and size prove the ACCESS_ALLOWED_ACE shape.
            let header = unsafe { &*entry.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
            let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
            if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
                || (header.AceSize as usize) < sid_offset + 8
            {
                return Err(invalid(
                    path,
                    "profile storage has an unsupported access entry",
                ));
            }
            let ace = unsafe { &*entry.cast::<ACCESS_ALLOWED_ACE>() };
            let sid = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
            // SAFETY: the SID header fits within the validated ACE; check its full
            // length before asking APIs to inspect or compare the complete SID.
            let sid_length = unsafe { GetLengthSid(sid) } as usize;
            if sid_length > ace.Header.AceSize as usize - sid_offset
                || unsafe { IsValidSid(sid) } == 0
            {
                return Err(invalid(path, "profile storage has an invalid access entry"));
            }
            let current = unsafe { EqualSid(sid, user) } != 0;
            if !current && unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } == 0 {
                return Err(invalid(
                    path,
                    "profile storage permissions must exclude other users",
                ));
            }
            let flags = ace.Header.AceFlags as u32;
            let full_access =
                ace.Mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS || ace.Mask & GENERIC_ALL != 0;
            let inheritable = flags & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
                == OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
                && flags & NO_PROPAGATE_INHERIT_ACE == 0;
            user_access |= current
                && full_access
                && flags & INHERIT_ONLY_ACE == 0
                && (!directory || inheritable);
        }
        if !user_access {
            return Err(invalid(
                path,
                if directory {
                    "data directory must grant inheritable current-user access"
                } else {
                    "profile storage must grant full current-user access"
                },
            ));
        }
        Ok(())
    }
}

struct User(Vec<usize>);

impl User {
    fn current() -> io::Result<Self> {
        let mut token = null_mut();
        // SAFETY: the process pseudo-handle is valid; token is writable.
        check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
        // SAFETY: OpenProcessToken supplied a new owning handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut length = 0;
        // SAFETY: a null buffer queries the required size without reading memory.
        unsafe {
            GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length)
        };
        if length < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        // SAFETY: usize storage is suitably aligned and holds the requested bytes.
        check(unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        })?;
        Ok(Self(buffer))
    }

    fn sid(&self) -> PSID {
        // SAFETY: the buffer is a successful TokenUser result, still alive here.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

struct Private {
    _user: User,
    _acl: Vec<usize>,
    descriptor: SECURITY_DESCRIPTOR,
}

impl Private {
    fn new() -> io::Result<Self> {
        let user = User::current()?;
        // SAFETY: the token result contains a valid SID.
        let length = size_of::<ACL>()
            + offset_of!(ACCESS_ALLOWED_ACE, SidStart)
            + unsafe { GetLengthSid(user.sid()) } as usize;
        let mut acl = vec![0usize; length.div_ceil(size_of::<usize>())];
        let dacl = acl.as_mut_ptr().cast();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let pointer = std::ptr::from_mut(&mut descriptor).cast();
        // SAFETY: all buffers are correctly aligned, sized, and remain alive;
        // owner and ACL point into heap allocations which do not move with Self.
        unsafe {
            check(InitializeAcl(dacl, length as u32, ACL_REVISION))?;
            check(AddAccessAllowedAceEx(
                dacl,
                ACL_REVISION,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                FILE_ALL_ACCESS,
                user.sid(),
            ))?;
            check(InitializeSecurityDescriptor(
                pointer,
                SECURITY_DESCRIPTOR_REVISION,
            ))?;
            check(SetSecurityDescriptorOwner(pointer, user.sid(), 0))?;
            check(SetSecurityDescriptorDacl(pointer, 1, dacl, 0))?;
            check(SetSecurityDescriptorControl(
                pointer,
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            ))?;
        }
        Ok(Self {
            _user: user,
            _acl: acl,
            descriptor,
        })
    }

    fn attributes(&mut self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::from_mut(&mut self.descriptor).cast(),
            bInheritHandle: 0,
        }
    }
}

struct Allocation(*mut std::ffi::c_void);

impl Drop for Allocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetSecurityInfo allocates the descriptor with LocalAlloc.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut name: Vec<_> = path.as_os_str().encode_wide().collect();
    if name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "profile path contains a null character",
        ));
    }
    name.push(0);
    Ok(name)
}

fn check(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
