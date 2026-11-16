#ifndef SAILRY_TURSO_H
#define SAILRY_TURSO_H

#include "turso.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Open a synchronous official SDK connection with index_method enabled.
 * Outputs are null on failure. Free errors with turso_str_deinit.
 * On success, the official Go SDK owns the connection; close the database
 * owner only after that connection and its cancellation callbacks finish. */
turso_status_code_t sailry_turso_open(const char *path,
                                    const turso_database_t **database_out,
                                    turso_connection_t **connection_out,
                                    const char **error_out);
void sailry_turso_database_close(const turso_database_t *database);

/* Thread-safe while connection remains alive. Join the caller's cancellation
 * callback before closing or returning the connection to its pool. */
void sailry_turso_connection_interrupt(const turso_connection_t *connection);

#ifdef __cplusplus
}
#endif

#endif
