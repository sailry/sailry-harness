package host

import (
	"slices"
	"sync"

	"github.com/ebitengine/purego"
)

// GPU load and mounted-volume backing devices are IOKit gaps in gopsutil.
// Keep their shared IOKit and CoreFoundation libraries open for the process.
type ioRegistry struct {
	matching    func(name string) uintptr
	services    func(port uint32, matching uintptr, iterator *uint32) int32
	bsdMatching func(port, options uint32, name string) uintptr
	service     func(port uint32, matching uintptr) uint32
	ancestors   func(entry uint32, plane string, options uint32, iterator *uint32) int32
	next        func(iterator uint32) uint32
	release     func(object uint32) int32
	entryID     func(entry uint32, id *uint64) int32
	property    func(entry uint32, key, allocator uintptr, options uint32) uintptr
	free        func(value uintptr)
	typeOf      func(value uintptr) uintptr
	stringType  func() uintptr
	numberType  func() uintptr
	tableType   func() uintptr
	newString   func(allocator uintptr, text string, encoding uint32) uintptr
	tableValue  func(table, key uintptr) uintptr
	number      func(value uintptr, kind int64, out *int64) uint8
	length      func(value uintptr) int64
	capacity    func(length int64, encoding uint32) int64
	text        func(value uintptr, buffer *byte, size int64, encoding uint32) uint8
}

const (
	registryEncodingUTF8 = 0x08000100
	registryNumberInt64  = 4
)

var loadRegistry = sync.OnceValues(func() (*ioRegistry, error) {
	foundation, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	iokit, err := purego.Dlopen("/System/Library/Frameworks/IOKit.framework/IOKit", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	registry := &ioRegistry{}
	for _, entry := range []struct {
		target  any
		library uintptr
		name    string
	}{
		{&registry.matching, iokit, "IOServiceMatching"}, {&registry.services, iokit, "IOServiceGetMatchingServices"},
		{&registry.bsdMatching, iokit, "IOBSDNameMatching"}, {&registry.service, iokit, "IOServiceGetMatchingService"},
		{&registry.ancestors, iokit, "IORegistryEntryCreateIterator"},
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

// read copies a property of a registry entry: the caller releases it.
func (registry *ioRegistry) read(entry uint32, name string) uintptr {
	key := registry.newString(0, name, registryEncodingUTF8)
	if key == 0 {
		return 0
	}
	defer registry.free(key)
	return registry.property(entry, key, 0, 0)
}

// stringProperty copies an owned CF string before releasing it. Registry
// display names use the same bound as the accelerator's model name.
func (registry *ioRegistry) stringProperty(entry uint32, name string) string {
	value := registry.read(entry, name)
	if value == 0 {
		return ""
	}
	defer registry.free(value)
	if registry.typeOf(value) != registry.stringType() {
		return ""
	}
	size := registry.capacity(registry.length(value), registryEncodingUTF8) + 1
	if size <= 1 || size > 1024 {
		return ""
	}
	buffer := make([]byte, size)
	if registry.text(value, &buffer[0], size, registryEncodingUTF8) == 0 {
		return ""
	}
	return string(buffer[:max(0, slices.Index(buffer, 0))])
}
