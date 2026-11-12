package host

import (
	"cmp"
	"slices"
	"strconv"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// gopsutil has no GPU sampler. macOS publishes each accelerator's load in
// the IOAccelerator registry entries, read through IOKit with purego as the
// reference's sampler does (host/metrics/gpu.rs).
type acceleratorRegistry struct {
	matching   func(name string) uintptr
	services   func(port uint32, matching uintptr, iterator *uint32) int32
	next       func(iterator uint32) uint32
	release    func(object uint32) int32
	entryID    func(entry uint32, id *uint64) int32
	property   func(entry uint32, key, allocator uintptr, options uint32) uintptr
	free       func(value uintptr)
	typeOf     func(value uintptr) uintptr
	stringType func() uintptr
	numberType func() uintptr
	tableType  func() uintptr
	newString  func(allocator uintptr, text string, encoding uint32) uintptr
	tableValue func(table, key uintptr) uintptr
	number     func(value uintptr, kind int64, out *int64) uint8
	length     func(value uintptr) int64
	capacity   func(length int64, encoding uint32) int64
	text       func(value uintptr, buffer *byte, size int64, encoding uint32) uint8
}

const (
	gpuEncodingUTF8 = 0x08000100
	gpuNumberInt64  = 4
	gpuLimit        = 32
)

var loadAccelerators = sync.OnceValues(func() (*acceleratorRegistry, error) {
	foundation, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	iokit, err := purego.Dlopen("/System/Library/Frameworks/IOKit.framework/IOKit", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	registry := &acceleratorRegistry{}
	for _, entry := range []struct {
		target  any
		library uintptr
		name    string
	}{
		{&registry.matching, iokit, "IOServiceMatching"}, {&registry.services, iokit, "IOServiceGetMatchingServices"},
		{&registry.next, iokit, "IOIteratorNext"}, {&registry.release, iokit, "IOObjectRelease"},
		{&registry.entryID, iokit, "IORegistryEntryGetRegistryEntryID"}, {&registry.property, iokit, "IORegistryEntryCreateCFProperty"},
		{&registry.free, foundation, "CFRelease"}, {&registry.typeOf, foundation, "CFGetTypeID"},
		{&registry.stringType, foundation, "CFStringGetTypeID"}, {&registry.numberType, foundation, "CFNumberGetTypeID"},
		{&registry.tableType, foundation, "CFDictionaryGetTypeID"}, {&registry.newString, foundation, "CFStringCreateWithCString"},
		{&registry.tableValue, foundation, "CFDictionaryGetValue"}, {&registry.number, foundation, "CFNumberGetValue"},
		{&registry.length, foundation, "CFStringGetLength"}, {&registry.capacity, foundation, "CFStringGetMaximumSizeForEncoding"},
		{&registry.text, foundation, "CFStringGetCString"},
	} {
		address, err := purego.Dlsym(entry.library, entry.name)
		if err != nil {
			return nil, err
		}
		purego.RegisterFunc(entry.target, address)
	}
	return registry, nil
})

// sampleGPUs lists the accelerators with their load, by registry ID: none
// when the registry cannot be read. A load the driver does not publish, or
// one outside 0 to 100, is unknown.
func sampleGPUs() []protocol.HostMetricsGpu {
	devices := []protocol.HostMetricsGpu{}
	registry, err := loadAccelerators()
	if err != nil {
		return devices
	}
	matching := registry.matching("IOAccelerator")
	if matching == 0 {
		return devices
	}
	// IOKit consumes the matching dictionary and returns an owned iterator.
	var iterator uint32
	if registry.services(0, matching, &iterator) != 0 {
		return devices
	}
	defer registry.release(iterator)
	for len(devices) < gpuLimit {
		entry := registry.next(iterator)
		if entry == 0 {
			break
		}
		if device, ok := registry.device(entry); ok {
			devices = append(devices, device)
		}
		registry.release(entry)
	}
	slices.SortFunc(devices, func(left, right protocol.HostMetricsGpu) int { return cmp.Compare(left.ID, right.ID) })
	return devices
}

func (registry *acceleratorRegistry) device(entry uint32) (protocol.HostMetricsGpu, bool) {
	var id uint64
	if registry.entryID(entry, &id) != 0 {
		return protocol.HostMetricsGpu{}, false
	}
	device := protocol.HostMetricsGpu{ID: strconv.FormatUint(id, 10), Name: "GPU"}
	if model := registry.read(entry, "model"); model != 0 {
		if registry.typeOf(model) == registry.stringType() {
			if size := registry.capacity(registry.length(model), gpuEncodingUTF8) + 1; size > 1 && size <= 1024 {
				buffer := make([]byte, size)
				if registry.text(model, &buffer[0], size, gpuEncodingUTF8) != 0 {
					if name := string(buffer[:max(0, slices.Index(buffer, 0))]); name != "" {
						device.Name = name
					}
				}
			}
		}
		registry.free(model)
	}
	if statistics := registry.read(entry, "PerformanceStatistics"); statistics != 0 {
		if registry.typeOf(statistics) == registry.tableType() {
			key := registry.newString(0, "Device Utilization %", gpuEncodingUTF8)
			// The dictionary owns the value it returns.
			if value := registry.tableValue(statistics, key); value != 0 && registry.typeOf(value) == registry.numberType() {
				var percent int64
				if registry.number(value, gpuNumberInt64, &percent) != 0 && percent >= 0 && percent <= 100 {
					usage := uint32(percent) * 100
					device.UsageBasisPoints = &usage
				}
			}
			registry.free(key)
		}
		registry.free(statistics)
	}
	return device, true
}

// read copies a property of a registry entry: the caller releases it.
func (registry *acceleratorRegistry) read(entry uint32, name string) uintptr {
	key := registry.newString(0, name, gpuEncodingUTF8)
	if key == 0 {
		return 0
	}
	defer registry.free(key)
	return registry.property(entry, key, 0, 0)
}
