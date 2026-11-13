package host

import (
	"slices"

	"github.com/shirou/gopsutil/v4/disk"
)

// physicalDisks fills gopsutil's APFS topology gap: a mounted volume or
// snapshot has its own BSD name, while counters belong to whole drives.
// The rates cover those drives, shared with their other volumes; macOS does
// not attribute them to this volume. No matching counter means unknown I/O.
func physicalDisks(device string, counters map[string]disk.IOCountersStat) []string {
	if len(counters) == 0 {
		return nil
	}
	registry, err := loadRegistry()
	if err != nil {
		return nil
	}
	return registry.physicalDisks(device, counters)
}

func (registry *ioRegistry) physicalDisks(device string, counters map[string]disk.IOCountersStat) []string {
	matching := registry.bsdMatching(0, 0, device)
	if matching == 0 {
		return nil
	}
	// The service lookup consumes the matching dictionary.
	entry := registry.service(0, matching)
	if entry == 0 {
		return nil
	}
	defer registry.release(entry)
	var iterator uint32
	// IOKit traverses every parent recursively, including multiple backing
	// stores, and avoids loops itself. A first-parent walk loses stores.
	const iterateAncestors = 0x1 | 0x2
	if registry.ancestors(entry, "IOService", iterateAncestors, &iterator) != 0 || iterator == 0 {
		return nil
	}
	defer registry.release(iterator)
	devices := []string{}
	for {
		ancestor := registry.next(iterator)
		if ancestor == 0 {
			break
		}
		name := registry.stringProperty(ancestor, "BSD Name")
		registry.release(ancestor)
		if _, exists := counters[name]; exists && !slices.Contains(devices, name) {
			devices = append(devices, name)
		}
	}
	slices.Sort(devices)
	return devices
}
