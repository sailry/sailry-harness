// The official Go SDK registers both ABIs even when it opens only local files.
// Feature unification enables the native FTS index used by the Rust database.
pub use turso_local::capi as local;
pub use turso_sync::capi as sync;

use local::c::{
    turso_connection_t, turso_database_config_t, turso_database_t, turso_status_code_t,
};
use std::{ffi::c_char, ptr};
use turso_local::rsapi::{TursoConnection, TursoError};

/// Open an official SDK connection whose owner can interrupt a running statement.
///
/// # Safety
/// `path` must be a valid UTF-8 C string. Output pointers must be writable;
/// `error_out` may be null. On success the caller owns both returned handles.
#[no_mangle]
pub unsafe extern "C" fn sailry_turso_open(
    path: *const c_char,
    database_out: *mut *const turso_database_t,
    connection_out: *mut *mut turso_connection_t,
    error_out: *mut *const c_char,
) -> turso_status_code_t {
    if !error_out.is_null() {
        *error_out = ptr::null();
    }
    if !database_out.is_null() {
        *database_out = ptr::null();
    }
    if !connection_out.is_null() {
        *connection_out = ptr::null_mut();
    }
    if database_out.is_null() || connection_out.is_null() {
        return TursoError::Misuse("database and connection outputs are required".into())
            .to_capi(error_out);
    }
    let config = turso_database_config_t {
        path,
        experimental_features: c"index_method".as_ptr(),
        ..Default::default()
    };
    let mut database = ptr::null();
    let mut status = local::turso_database_new(&config, &mut database, error_out);
    if status != turso_status_code_t::TURSO_OK {
        return status;
    }
    status = local::turso_database_open(database, error_out);
    if status == turso_status_code_t::TURSO_OK {
        status = local::turso_database_connect(database, connection_out, error_out);
    }
    if status != turso_status_code_t::TURSO_OK {
        local::turso_database_deinit(database);
        return status;
    }
    *database_out = database;
    status
}

/// Release the database owner after the official Go connection has closed.
///
/// # Safety
/// `database` must be null or an owned handle returned by `sailry_turso_open`.
#[no_mangle]
pub unsafe extern "C" fn sailry_turso_database_close(database: *const turso_database_t) {
    local::turso_database_deinit(database);
}

/// Interrupt a running statement through the official thread-safe Rust API.
///
/// # Safety
/// The connection must remain alive until this call has returned. The caller
/// must join cancellation callbacks before closing or reusing the connection.
#[no_mangle]
pub unsafe extern "C" fn sailry_turso_connection_interrupt(connection: *const turso_connection_t) {
    if let Ok(connection) = TursoConnection::ref_from_capi(connection) {
        connection.interrupt();
    }
}
