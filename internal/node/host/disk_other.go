//go:build !darwin

package host

import "github.com/shirou/gopsutil/v4/disk"

func physicalDisks(string, map[string]disk.IOCountersStat) []string { return nil }
