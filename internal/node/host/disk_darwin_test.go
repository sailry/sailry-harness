package host

import (
	"reflect"
	"slices"
	"testing"
	"unsafe"

	"github.com/shirou/gopsutil/v4/disk"
)

func TestPhysicalDisks(t *testing.T) {
	t.Run("unavailable counters need no topology lookup", func(t *testing.T) {
		if got := physicalDisks("disk3s1s1", nil); got != nil {
			t.Fatalf("unavailable counters produced %v", got)
		}
	})
	t.Run("snapshot follows all backing stores and releases registry objects", func(t *testing.T) {
		names := map[uint32]string{11: "disk3s1", 12: "disk3", 13: "disk0s2", 14: "disk0", 15: "disk4", 16: "disk0"}
		ancestors := []uint32{10, 11, 12, 13, 14, 15, 16}
		next := 0
		var released []uint32
		freed := map[uintptr]int{}
		registry := &ioRegistry{
			bsdMatching: func(port, options uint32, name string) uintptr {
				if port != 0 || options != 0 || name != "disk3s1s1" {
					t.Fatalf("BSD lookup changed the snapshot: %v %v %q", port, options, name)
				}
				return 1
			},
			service: func(port uint32, matching uintptr) uint32 {
				if port != 0 || matching != 1 {
					t.Fatalf("service lookup received %v %v", port, matching)
				}
				return 2
			},
			ancestors: func(entry uint32, plane string, options uint32, iterator *uint32) int32 {
				if entry != 2 || plane != "IOService" || options != 3 {
					t.Fatalf("ancestor traversal received %v %q %v", entry, plane, options)
				}
				*iterator = 3
				return 0
			},
			next: func(iterator uint32) uint32 {
				if iterator != 3 {
					t.Fatalf("unexpected iterator %v", iterator)
				}
				if next == len(ancestors) {
					return 0
				}
				entry := ancestors[next]
				next++
				return entry
			},
			release: func(object uint32) int32 { released = append(released, object); return 0 },
			newString: func(allocator uintptr, name string, encoding uint32) uintptr {
				if allocator != 0 || name != "BSD Name" || encoding != registryEncodingUTF8 {
					t.Fatalf("unexpected registry property %q", name)
				}
				return 4
			},
			property: func(entry uint32, key, allocator uintptr, options uint32) uintptr {
				if key != 4 || allocator != 0 || options != 0 {
					t.Fatal("invalid property lookup")
				}
				if _, exists := names[entry]; exists {
					return uintptr(entry)
				}
				return 0
			},
			free:       func(value uintptr) { freed[value]++ },
			typeOf:     func(uintptr) uintptr { return 5 },
			stringType: func() uintptr { return 5 },
			length:     func(value uintptr) int64 { return int64(len(names[uint32(value)])) },
			capacity:   func(length int64, _ uint32) int64 { return length * 3 },
			text: func(value uintptr, buffer *byte, size int64, _ uint32) uint8 {
				copy(unsafe.Slice(buffer, int(size)), names[uint32(value)]+"\x00")
				return 1
			},
		}
		counters := map[string]disk.IOCountersStat{"disk0": {}, "disk4": {}, "disk9": {}}
		if got := registry.physicalDisks("disk3s1s1", counters); !reflect.DeepEqual(got, []string{"disk0", "disk4"}) {
			t.Fatalf("backing disks %v, want only the matching whole-drive counters", got)
		}
		if want := append(slices.Clone(ancestors), 3, 2); !reflect.DeepEqual(released, want) {
			t.Fatalf("released registry objects %v, want %v", released, want)
		}
		if freed[1] != 0 || freed[4] != len(ancestors) {
			t.Fatal("the matching dictionary was released again or a property key leaked")
		}
		for entry := range names {
			if freed[uintptr(entry)] != 1 {
				t.Errorf("property %v released %v times", entry, freed[uintptr(entry)])
			}
		}
	})
	t.Run("unavailable topology stays unknown", func(t *testing.T) {
		for _, stage := range []string{"matching", "service", "ancestors"} {
			t.Run(stage, func(t *testing.T) {
				released := []uint32{}
				registry := &ioRegistry{
					bsdMatching: func(uint32, uint32, string) uintptr {
						if stage == "matching" {
							return 0
						}
						return 1
					},
					service: func(uint32, uintptr) uint32 {
						if stage == "service" {
							return 0
						}
						return 2
					},
					ancestors: func(uint32, string, uint32, *uint32) int32 { return -1 },
					release:   func(object uint32) int32 { released = append(released, object); return 0 },
				}
				if got := registry.physicalDisks("disk3s1s1", map[string]disk.IOCountersStat{"disk0": {}}); got != nil {
					t.Fatalf("unavailable topology produced %v", got)
				}
				if len(released) != 0 && stage != "ancestors" || stage == "ancestors" && !reflect.DeepEqual(released, []uint32{2}) {
					t.Fatalf("failed %s lookup released %v", stage, released)
				}
			})
		}
	})
}
