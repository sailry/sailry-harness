package host

import (
	"cmp"
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// gopsutil has no GPU sampler. macOS publishes each accelerator's load in
// the IOAccelerator registry entries, read through IOKit with purego as the
// reference's sampler does (host/metrics/gpu.rs).
const gpuLimit = 32

// sampleGPUs lists the accelerators with their load, by registry ID: none
// when the registry cannot be read. A load the driver does not publish, or
// one outside 0 to 100, is unknown.
func sampleGPUs() []protocol.HostMetricsGpu {
	devices := []protocol.HostMetricsGpu{}
	registry, err := loadRegistry()
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

func (registry *ioRegistry) device(entry uint32) (protocol.HostMetricsGpu, bool) {
	var id uint64
	if registry.entryID(entry, &id) != 0 {
		return protocol.HostMetricsGpu{}, false
	}
	device := protocol.HostMetricsGpu{ID: strconv.FormatUint(id, 10), Name: "GPU"}
	if model := registry.stringProperty(entry, "model"); model != "" {
		device.Name = model
	}
	if statistics := registry.read(entry, "PerformanceStatistics"); statistics != 0 {
		if registry.typeOf(statistics) == registry.tableType() {
			key := registry.newString(0, "Device Utilization %", registryEncodingUTF8)
			// The dictionary owns the value it returns.
			if value := registry.tableValue(statistics, key); value != 0 && registry.typeOf(value) == registry.numberType() {
				var percent int64
				if registry.number(value, registryNumberInt64, &percent) != 0 && percent >= 0 && percent <= 100 {
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
