#ifndef SAILRY_MOBILE_H
#define SAILRY_MOBILE_H

#include <stdint.h>

typedef void (*sailry_callback)(uint64_t request, uint8_t *data, uint64_t length);

uint64_t sailry_open(sailry_callback callback);
uint64_t sailry_invoke(uint64_t bridge, uint64_t handle, const char *op,
                       const uint8_t *json, uint64_t length, uint64_t request,
                       uint8_t **immediate, uint64_t *immediate_length);
uint64_t sailry_subscribe(uint64_t bridge, uint64_t handle, const char *op,
                          const uint8_t *json, uint64_t length, uint64_t request);
void sailry_cancel(uint64_t bridge, uint64_t request);
void sailry_free(uint8_t *data);
void sailry_close(uint64_t bridge, uint64_t request);

#endif
