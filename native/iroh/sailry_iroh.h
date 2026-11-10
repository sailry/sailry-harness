#ifndef SAILRY_IROH_H
#define SAILRY_IROH_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*sailry_iroh_complete)(uintptr_t, uintptr_t, const uint8_t *, uintptr_t);

int32_t sailry_iroh_init(sailry_iroh_complete callback);
uintptr_t sailry_iroh_default_relay_urls(void);
uintptr_t sailry_iroh_endpoint_new(const uint8_t *config, uintptr_t length);
uintptr_t sailry_iroh_endpoint_addr(uint64_t endpoint);
uintptr_t sailry_iroh_endpoint_close(uint64_t endpoint);
uintptr_t sailry_iroh_endpoint_online(uint64_t endpoint);
uintptr_t sailry_iroh_endpoint_network_change(uint64_t endpoint);
uintptr_t sailry_iroh_endpoint_insert_relay(uint64_t endpoint, const uint8_t *url, uintptr_t length);
uintptr_t sailry_iroh_endpoint_remove_relay(uint64_t endpoint, const uint8_t *url, uintptr_t length);
uintptr_t sailry_iroh_endpoint_relay_status(uint64_t endpoint);
uintptr_t sailry_iroh_endpoint_wait_relay_status(uint64_t endpoint, const uint8_t *previous, uintptr_t length);
uintptr_t sailry_iroh_endpoint_wait_address(uint64_t endpoint, const uint8_t *previous, uintptr_t length);
uintptr_t sailry_iroh_connect(uint64_t endpoint, const uint8_t *address, uintptr_t address_length, const uint8_t *alpn, uintptr_t alpn_length);
uintptr_t sailry_iroh_accept(uint64_t endpoint);
uintptr_t sailry_iroh_open_bi(uint64_t connection);
uintptr_t sailry_iroh_accept_bi(uint64_t connection);
uintptr_t sailry_iroh_send_write(uint64_t send, const uint8_t *data, uintptr_t length);
uintptr_t sailry_iroh_send_finish(uint64_t send);
uintptr_t sailry_iroh_send_stopped(uint64_t send);
uintptr_t sailry_iroh_send_reset(uint64_t send, uint64_t code);
uintptr_t sailry_iroh_recv_read(uint64_t receive, uintptr_t maximum);
uintptr_t sailry_iroh_recv_stop(uint64_t receive, uint64_t code);
uintptr_t sailry_iroh_conn_info(uint64_t connection);
uintptr_t sailry_iroh_conn_wait_path(uint64_t connection);
uintptr_t sailry_iroh_conn_close(uint64_t connection, uint64_t code, const uint8_t *reason, uintptr_t length);
uintptr_t sailry_iroh_stream_drop(uint64_t send, uint64_t receive);
void sailry_iroh_cancel(uintptr_t operation);
void sailry_iroh_free(const uint8_t *data, uintptr_t length);
uintptr_t sailry_iroh_debug_count(uintptr_t kind);

#ifdef __cplusplus
}
#endif

#endif
