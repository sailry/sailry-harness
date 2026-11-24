#ifndef SAILRY_MOBILE_H
#define SAILRY_MOBILE_H

#include <stdint.h>

// Results arrive on the Dart port as [request, Uint8List] messages; `api` is
// NativeApi.initializeApiDLData. A null message may probe the port's liveness.
uint64_t sailry_open(void *api, int64_t port);
uint64_t sailry_invoke(uint64_t bridge, uint64_t handle, const char *op,
                       const uint8_t *json, uint64_t length, uint64_t request,
                       uint8_t **immediate, uint64_t *immediate_length);
uint64_t sailry_subscribe(uint64_t bridge, uint64_t handle, const char *op,
                          const uint8_t *json, uint64_t length, uint64_t request);
void sailry_cancel(uint64_t bridge, uint64_t request);
void sailry_free(uint8_t *data);
void sailry_close(uint64_t bridge, uint64_t request);

#endif
