//! SQLx exposes its locked native handle, but not SQLite's statement safety metadata.
//! Preparation here classifies one statement; SQLx owns connection I/O and execution.
use super::*;
use libsqlite3_sys as ffi;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};

struct Prepared(*mut ffi::sqlite3_stmt);
impl Drop for Prepared {
    fn drop(&mut self) {
        // SAFETY: preparation transfers this statement to the guard exactly once.
        unsafe {
            ffi::sqlite3_finalize(self.0);
        }
    }
}

pub(super) async fn inspect(
    connection: &mut SqliteConnection,
    sql: &str,
    classify: bool,
) -> Result<bool, Fault> {
    let sql = CString::new(sql).map_err(query_error)?;
    let mut locked = connection.lock_handle().await.map_err(query_error)?;
    let handle = locked.as_raw_handle().as_ptr();
    if classify {
        // SAFETY: SQLx excludes its worker while locked. The callback is static,
        // retains no pointers, and is removed before releasing this handle.
        let result =
            unsafe { ffi::sqlite3_set_authorizer(handle, Some(authorize), ptr::null_mut()) };
        if result != ffi::SQLITE_OK {
            return Err(error(handle, result, classify));
        }
    }
    let result = inspect_locked(handle, &sql, classify);
    if classify {
        // SAFETY: the handle is still exclusively locked and the callback owns no data.
        unsafe {
            ffi::sqlite3_set_authorizer(handle, None, ptr::null_mut());
        }
    }
    result
}

fn inspect_locked(handle: *mut ffi::sqlite3, sql: &CStr, classify: bool) -> Result<bool, Fault> {
    let mut cursor = sql.as_ptr();
    let mut first = None;
    let mut write = false;
    loop {
        let mut statement = ptr::null_mut();
        let mut tail = ptr::null();
        // SAFETY: cursor points inside the NUL-terminated SQL buffer; the exclusive
        // SQLx lock outlives every prepared statement and all native metadata reads.
        let result =
            unsafe { ffi::sqlite3_prepare_v3(handle, cursor, -1, 0, &mut statement, &mut tail) };
        let prepared = Prepared(statement);
        if result != ffi::SQLITE_OK {
            return Err(error(handle, result, classify));
        }
        if !statement.is_null() {
            if first.is_some() {
                return Err(query_error("Run one SQL statement at a time"));
            }
            // SAFETY: successful preparation returned a live statement guarded above.
            write = unsafe {
                ffi::sqlite3_stmt_readonly(statement) == 0
                    && (!classify || ffi::sqlite3_stmt_isexplain(statement) == 0)
            };
            first = Some(prepared);
        }
        // SAFETY: SQLite returns a tail inside the live NUL-terminated input buffer.
        if tail.is_null() || unsafe { *tail == 0 } {
            break;
        }
        cursor = tail;
    }
    first.ok_or_else(|| query_error("SQL statement is empty"))?;
    Ok(write)
}

fn error(handle: *mut ffi::sqlite3, code: c_int, classify: bool) -> Fault {
    if classify && code == ffi::SQLITE_AUTH {
        return missing();
    }
    // SAFETY: SQLite retains this NUL-terminated diagnostic while the handle is locked.
    query_error(unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(handle)) }.to_string_lossy())
}

unsafe extern "C" fn authorize(
    _: *mut c_void,
    action: c_int,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action == ffi::SQLITE_PRAGMA {
        ffi::SQLITE_DENY
    } else {
        ffi::SQLITE_OK
    }
}
