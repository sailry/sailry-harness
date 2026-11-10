package web

import (
	"net"
	"strconv"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
)

type proxyFoundation struct {
	copyProxies                       func(uintptr) uintptr
	release                           func(uintptr)
	getType                           func(uintptr) uintptr
	stringType, numberType, arrayType func() uintptr
	newString                         func(uintptr, string, uint32) uintptr
	dictionaryValue                   func(uintptr, uintptr) uintptr
	numberValue                       func(uintptr, int64, *int32) uint8
	stringLength                      func(uintptr) int64
	stringCapacity                    func(int64, uint32) int64
	stringBytes                       func(uintptr, *byte, int64, uint32) uint8
	arrayCount                        func(uintptr) int64
	arrayValue                        func(uintptr, int64) uintptr
}

var loadProxyFoundation = sync.OnceValues(func() (*proxyFoundation, error) {
	configuration, err := purego.Dlopen("/System/Library/Frameworks/SystemConfiguration.framework/SystemConfiguration", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	foundation, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_LOCAL)
	if err != nil {
		return nil, err
	}
	functions := &proxyFoundation{}
	for _, entry := range []struct {
		target  any
		library uintptr
		name    string
	}{
		{&functions.copyProxies, configuration, "SCDynamicStoreCopyProxies"},
		{&functions.release, foundation, "CFRelease"}, {&functions.getType, foundation, "CFGetTypeID"},
		{&functions.stringType, foundation, "CFStringGetTypeID"}, {&functions.numberType, foundation, "CFNumberGetTypeID"}, {&functions.arrayType, foundation, "CFArrayGetTypeID"},
		{&functions.newString, foundation, "CFStringCreateWithCString"}, {&functions.dictionaryValue, foundation, "CFDictionaryGetValue"},
		{&functions.numberValue, foundation, "CFNumberGetValue"}, {&functions.stringLength, foundation, "CFStringGetLength"}, {&functions.stringCapacity, foundation, "CFStringGetMaximumSizeForEncoding"}, {&functions.stringBytes, foundation, "CFStringGetCString"},
		{&functions.arrayCount, foundation, "CFArrayGetCount"}, {&functions.arrayValue, foundation, "CFArrayGetValueAtIndex"},
	} {
		address, err := purego.Dlsym(entry.library, entry.name)
		if err != nil {
			return nil, err
		}
		purego.RegisterFunc(entry.target, address)
	}
	return functions, nil
})

func nativeProxySettings() proxySettings {
	functions, err := loadProxyFoundation()
	if err != nil {
		return proxySettings{}
	}
	values := functions.copyProxies(0)
	if values == 0 {
		return proxySettings{}
	}
	defer functions.release(values)
	get := func(name string) uintptr {
		key := functions.newString(0, name, 0x08000100)
		if key == 0 {
			return 0
		}
		defer functions.release(key)
		return functions.dictionaryValue(values, key)
	}
	number := func(name string) (int32, bool) {
		value := get(name)
		if value == 0 || functions.getType(value) != functions.numberType() {
			return 0, false
		}
		var number int32
		ok := functions.numberValue(value, 3, &number) != 0
		return number, ok
	}
	text := func(value uintptr) string {
		if value == 0 || functions.getType(value) != functions.stringType() {
			return ""
		}
		capacity := functions.stringCapacity(functions.stringLength(value), 0x08000100) + 1
		if capacity <= 0 {
			return ""
		}
		buffer := make([]byte, capacity)
		if functions.stringBytes(value, &buffer[0], capacity, 0x08000100) == 0 {
			return ""
		}
		return strings.TrimRight(string(buffer), "\x00")
	}
	endpoint := func(scheme string) string {
		enabled, ok := number(scheme + "Enable")
		if !ok || enabled != 1 {
			return ""
		}
		host := text(get(scheme + "Proxy"))
		port, ok := number(scheme + "Port")
		if !ok || port <= 0 || port > 65535 || host == "" {
			return ""
		}
		return "http://" + net.JoinHostPort(host, strconv.Itoa(int(port)))
	}
	settings := proxySettings{http: endpoint("HTTP"), https: endpoint("HTTPS")}
	if array := get("ExceptionsList"); array != 0 && functions.getType(array) == functions.arrayType() {
		entries := []string{}
		for index, count := int64(0), functions.arrayCount(array); index < count; index++ {
			if value := text(functions.arrayValue(array, index)); value != "" {
				entries = append(entries, strings.TrimPrefix(value, "*."))
			}
		}
		settings.bypass = strings.Join(entries, ",")
	}
	return settings
}
