// Flat microphone capture API over miniaudio 0.11.26 for a cgo-free Go host.
//
// miniaudio configuration structures are large by-value types whose layout
// depends on the platform. This shim keeps them in C, moves captured frames
// through a lock-free single-producer single-consumer ring buffer and exports
// only pointers and integers, so the audio thread never enters Go.
//
// A device is named by the hexadecimal form of its miniaudio device ID with
// trailing zero bytes removed. The ID belongs to the backend miniaudio chose
// (Core Audio UID, WASAPI endpoint ID, ALSA or PulseAudio name).
//
// Build: see build.sh and README.md.
#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_GENERATION
#define MA_NO_ENGINE
#define MA_NO_NODE_GRAPH
#define MA_NO_RESOURCE_MANAGER
#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"

#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define SAILRY_API __declspec(dllexport)
#else
#define SAILRY_API __attribute__((visibility("default")))
#endif

typedef struct {
    ma_context context;
    ma_device device;
    ma_pcm_rb buffer;
    ma_uint32 channels;
    ma_atomic_uint32 dropped;
    ma_atomic_uint32 failed;
    ma_atomic_uint32 stopping;
} sailry_capture;

SAILRY_API const char* sailry_capture_version(void) { return MA_VERSION_STRING; }

static int sailry_capture_digit(char value) {
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    return -1;
}

static ma_bool32 sailry_capture_identifier(const char* text, ma_device_id* id) {
    size_t length = strlen(text);
    unsigned char* bytes = (unsigned char*)id;
    memset(id, 0, sizeof(*id));
    if (length == 0 || length % 2 != 0 || length / 2 > sizeof(*id)) {
        return MA_FALSE;
    }
    for (size_t index = 0; index < length; index += 2) {
        int high = sailry_capture_digit(text[index]);
        int low = sailry_capture_digit(text[index + 1]);
        if (high < 0 || low < 0) {
            return MA_FALSE;
        }
        bytes[index / 2] = (unsigned char)(high << 4 | low);
    }
    return MA_TRUE;
}

// Writes "<1 when default>\t<device>\t<name>\n" for every capture device and
// returns the device count, or a negative miniaudio result. Devices that do
// not fit the output are omitted from it.
SAILRY_API int sailry_capture_devices(char* output, size_t capacity) {
    static const char digits[] = "0123456789abcdef";
    ma_context context;
    ma_device_info* devices;
    ma_uint32 count;
    size_t used = 0;
    ma_result result = ma_context_init(NULL, 0, NULL, &context);
    if (result != MA_SUCCESS) {
        return result;
    }
    result = ma_context_get_devices(&context, NULL, NULL, &devices, &count);
    if (result != MA_SUCCESS) {
        ma_context_uninit(&context);
        return result;
    }
    if (capacity > 0) {
        output[0] = '\0';
    }
    for (ma_uint32 index = 0; index < count; index++) {
        const unsigned char* bytes = (const unsigned char*)&devices[index].id;
        size_t size = sizeof(devices[index].id);
        size_t length = strlen(devices[index].name);
        while (size > 0 && bytes[size - 1] == 0) {
            size--;
        }
        if (used + size * 2 + length + 5 > capacity) {
            break;
        }
        output[used++] = devices[index].isDefault ? '1' : '0';
        output[used++] = '\t';
        for (size_t byte = 0; byte < size; byte++) {
            output[used++] = digits[bytes[byte] >> 4];
            output[used++] = digits[bytes[byte] & 15];
        }
        output[used++] = '\t';
        for (size_t character = 0; character < length; character++) {
            char value = devices[index].name[character];
            output[used++] = value == '\t' || value == '\n' ? ' ' : value;
        }
        output[used++] = '\n';
        output[used] = '\0';
    }
    ma_context_uninit(&context);
    return (int)count;
}

static void sailry_capture_data(ma_device* device, void* output, const void* input, ma_uint32 frames) {
    sailry_capture* capture = (sailry_capture*)device->pUserData;
    const float* source = (const float*)input;
    (void)output;
    while (frames > 0) {
        void* target;
        ma_uint32 count = frames;
        if (ma_pcm_rb_acquire_write(&capture->buffer, &count, &target) != MA_SUCCESS || count == 0) {
            ma_atomic_uint32_fetch_add(&capture->dropped, frames);
            return;
        }
        memcpy(target, source, (size_t)count * capture->channels * sizeof(float));
        ma_pcm_rb_commit_write(&capture->buffer, count);
        source += (size_t)count * capture->channels;
        frames -= count;
    }
}

static void sailry_capture_notification(const ma_device_notification* notification) {
    sailry_capture* capture = (sailry_capture*)notification->pDevice->pUserData;
    if (notification->type == ma_device_notification_type_stopped && ma_atomic_uint32_get(&capture->stopping) == 0) {
        ma_atomic_uint32_set(&capture->failed, 1);
    }
}

// Opens a capture device as 32-bit float at its native rate and channel
// count: the default device when `device` is NULL, otherwise the one named by
// sailry_capture_devices. Returns NULL on failure and stores the miniaudio
// result in *result.
SAILRY_API sailry_capture* sailry_capture_open(const char* device, int* result) {
    sailry_capture* capture;
    ma_device_config config;
    ma_device_id id;
    if (device != NULL && !sailry_capture_identifier(device, &id)) {
        *result = MA_INVALID_ARGS;
        return NULL;
    }
    capture = (sailry_capture*)calloc(1, sizeof(sailry_capture));
    if (capture == NULL) {
        *result = MA_OUT_OF_MEMORY;
        return NULL;
    }
    *result = ma_context_init(NULL, 0, NULL, &capture->context);
    if (*result != MA_SUCCESS) {
        free(capture);
        return NULL;
    }
    config = ma_device_config_init(ma_device_type_capture);
    config.capture.format = ma_format_f32;
    config.capture.pDeviceID = device != NULL ? &id : NULL;
    config.dataCallback = sailry_capture_data;
    config.notificationCallback = sailry_capture_notification;
    config.pUserData = capture;
    *result = ma_device_init(&capture->context, &config, &capture->device);
    if (*result != MA_SUCCESS) {
        ma_context_uninit(&capture->context);
        free(capture);
        return NULL;
    }
    capture->channels = capture->device.capture.channels;
    // Two seconds of audio between reads.
    *result = ma_pcm_rb_init(ma_format_f32, capture->channels, capture->device.sampleRate * 2, NULL, NULL, &capture->buffer);
    if (*result != MA_SUCCESS) {
        ma_device_uninit(&capture->device);
        ma_context_uninit(&capture->context);
        free(capture);
        return NULL;
    }
    return capture;
}

SAILRY_API ma_uint32 sailry_capture_sample_rate(sailry_capture* capture) { return capture->device.sampleRate; }
SAILRY_API ma_uint32 sailry_capture_channels(sailry_capture* capture) { return capture->channels; }
SAILRY_API ma_uint32 sailry_capture_dropped(sailry_capture* capture) { return ma_atomic_uint32_get(&capture->dropped); }

// Reports 1 once the device stopped without sailry_capture_stop.
SAILRY_API ma_uint32 sailry_capture_failed(sailry_capture* capture) { return ma_atomic_uint32_get(&capture->failed); }

SAILRY_API int sailry_capture_start(sailry_capture* capture) { return ma_device_start(&capture->device); }
SAILRY_API int sailry_capture_stop(sailry_capture* capture) {
    ma_atomic_uint32_set(&capture->stopping, 1);
    return ma_device_stop(&capture->device);
}

// Copies up to `frames` interleaved frames and returns the number copied.
SAILRY_API ma_uint32 sailry_capture_read(sailry_capture* capture, float* output, ma_uint32 frames) {
    ma_uint32 total = 0;
    while (total < frames) {
        void* source;
        ma_uint32 count = frames - total;
        if (ma_pcm_rb_acquire_read(&capture->buffer, &count, &source) != MA_SUCCESS || count == 0) {
            break;
        }
        memcpy(output + (size_t)total * capture->channels, source, (size_t)count * capture->channels * sizeof(float));
        ma_pcm_rb_commit_read(&capture->buffer, count);
        total += count;
    }
    return total;
}

SAILRY_API void sailry_capture_close(sailry_capture* capture) {
    ma_atomic_uint32_set(&capture->stopping, 1);
    ma_device_uninit(&capture->device);
    ma_pcm_rb_uninit(&capture->buffer);
    ma_context_uninit(&capture->context);
    free(capture);
}
