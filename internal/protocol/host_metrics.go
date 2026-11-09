// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type HostMetricsSample struct {
	Node            NodeID                  `json:"node"`
	SampledAtMs     uint64                  `json:"sampled_at_ms"`
	CpuBasisPoints  *uint32                 `json:"cpu_basis_points"`
	CoreBasisPoints []uint32                `json:"core_basis_points"`
	Load            *HostMetricsLoadAverage `json:"load"`
	Memory          *MemoryUsage            `json:"memory"`
	Swap            *HostMetricsSwapUsage   `json:"swap"`
	Gpus            []HostMetricsGpu        `json:"gpus"`
	Disks           []HostMetricsDisk       `json:"disks"`
	Network         *HostMetricsNetworkRate `json:"network"`
	Processes       []HostMetricsProcess    `json:"processes"`
	ProcessCount    uint64                  `json:"process_count"`
}

func DefaultHostMetricsSample() HostMetricsSample {
	return HostMetricsSample{
		Node:            NodeID{},
		SampledAtMs:     0,
		CpuBasisPoints:  nil,
		CoreBasisPoints: []uint32{},
		Load:            nil,
		Memory:          nil,
		Swap:            nil,
		Gpus:            []HostMetricsGpu{},
		Disks:           []HostMetricsDisk{},
		Network:         nil,
		Processes:       []HostMetricsProcess{},
		ProcessCount:    0,
	}
}
func (value *HostMetricsSample) UnmarshalJSON(data []byte) error {
	type plain HostMetricsSample
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "sampled_at_ms", "core_basis_points", "gpus", "disks", "processes", "process_count"}, false); err != nil {
		return err
	}
	*value = HostMetricsSample(decoded)
	return nil
}
func (value HostMetricsSample) MarshalJSON() ([]byte, error) {
	type plain HostMetricsSample
	encoded := plain(value)
	encoded.CoreBasisPoints = normalizeCollections(encoded.CoreBasisPoints)
	encoded.Gpus = normalizeCollections(encoded.Gpus)
	encoded.Disks = normalizeCollections(encoded.Disks)
	encoded.Processes = normalizeCollections(encoded.Processes)
	return json.Marshal(encoded)
}

type HostMetricsLoadAverage struct {
	One     uint32 `json:"one"`
	Five    uint32 `json:"five"`
	Fifteen uint32 `json:"fifteen"`
}

func DefaultHostMetricsLoadAverage() HostMetricsLoadAverage {
	return HostMetricsLoadAverage{
		One:     0,
		Five:    0,
		Fifteen: 0,
	}
}
func (value *HostMetricsLoadAverage) UnmarshalJSON(data []byte) error {
	type plain HostMetricsLoadAverage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"one", "five", "fifteen"}, false); err != nil {
		return err
	}
	*value = HostMetricsLoadAverage(decoded)
	return nil
}
func (value HostMetricsLoadAverage) MarshalJSON() ([]byte, error) {
	type plain HostMetricsLoadAverage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsSwapUsage struct {
	TotalBytes uint64 `json:"total_bytes"`
	UsedBytes  uint64 `json:"used_bytes"`
}

func DefaultHostMetricsSwapUsage() HostMetricsSwapUsage {
	return HostMetricsSwapUsage{
		TotalBytes: 0,
		UsedBytes:  0,
	}
}
func (value *HostMetricsSwapUsage) UnmarshalJSON(data []byte) error {
	type plain HostMetricsSwapUsage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"total_bytes", "used_bytes"}, false); err != nil {
		return err
	}
	*value = HostMetricsSwapUsage(decoded)
	return nil
}
func (value HostMetricsSwapUsage) MarshalJSON() ([]byte, error) {
	type plain HostMetricsSwapUsage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsGpu struct {
	ID               string  `json:"id"`
	Name             string  `json:"name"`
	UsageBasisPoints *uint32 `json:"usage_basis_points"`
}

func DefaultHostMetricsGpu() HostMetricsGpu {
	return HostMetricsGpu{
		ID:               "",
		Name:             "",
		UsageBasisPoints: nil,
	}
}
func (value *HostMetricsGpu) UnmarshalJSON(data []byte) error {
	type plain HostMetricsGpu
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "name"}, false); err != nil {
		return err
	}
	*value = HostMetricsGpu(decoded)
	return nil
}
func (value HostMetricsGpu) MarshalJSON() ([]byte, error) {
	type plain HostMetricsGpu
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsDisk struct {
	Mount          string             `json:"mount"`
	Name           string             `json:"name"`
	FileSystem     string             `json:"file_system"`
	TotalBytes     uint64             `json:"total_bytes"`
	AvailableBytes uint64             `json:"available_bytes"`
	Io             *HostMetricsIoRate `json:"io"`
}

func DefaultHostMetricsDisk() HostMetricsDisk {
	return HostMetricsDisk{
		Mount:          "",
		Name:           "",
		FileSystem:     "",
		TotalBytes:     0,
		AvailableBytes: 0,
		Io:             nil,
	}
}
func (value *HostMetricsDisk) UnmarshalJSON(data []byte) error {
	type plain HostMetricsDisk
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"mount", "name", "file_system", "total_bytes", "available_bytes"}, false); err != nil {
		return err
	}
	*value = HostMetricsDisk(decoded)
	return nil
}
func (value HostMetricsDisk) MarshalJSON() ([]byte, error) {
	type plain HostMetricsDisk
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsIoRate struct {
	ReadBytesPerSec    uint64 `json:"read_bytes_per_sec"`
	WrittenBytesPerSec uint64 `json:"written_bytes_per_sec"`
}

func DefaultHostMetricsIoRate() HostMetricsIoRate {
	return HostMetricsIoRate{
		ReadBytesPerSec:    0,
		WrittenBytesPerSec: 0,
	}
}
func (value *HostMetricsIoRate) UnmarshalJSON(data []byte) error {
	type plain HostMetricsIoRate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"read_bytes_per_sec", "written_bytes_per_sec"}, false); err != nil {
		return err
	}
	*value = HostMetricsIoRate(decoded)
	return nil
}
func (value HostMetricsIoRate) MarshalJSON() ([]byte, error) {
	type plain HostMetricsIoRate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsNetworkRate struct {
	ReceivedBytesPerSec    uint64 `json:"received_bytes_per_sec"`
	TransmittedBytesPerSec uint64 `json:"transmitted_bytes_per_sec"`
	ReceivedBytes          uint64 `json:"received_bytes"`
	TransmittedBytes       uint64 `json:"transmitted_bytes"`
}

func DefaultHostMetricsNetworkRate() HostMetricsNetworkRate {
	return HostMetricsNetworkRate{
		ReceivedBytesPerSec:    0,
		TransmittedBytesPerSec: 0,
		ReceivedBytes:          0,
		TransmittedBytes:       0,
	}
}
func (value *HostMetricsNetworkRate) UnmarshalJSON(data []byte) error {
	type plain HostMetricsNetworkRate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"received_bytes_per_sec", "transmitted_bytes_per_sec", "received_bytes", "transmitted_bytes"}, false); err != nil {
		return err
	}
	*value = HostMetricsNetworkRate(decoded)
	return nil
}
func (value HostMetricsNetworkRate) MarshalJSON() ([]byte, error) {
	type plain HostMetricsNetworkRate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostMetricsProcess struct {
	Pid            uint32  `json:"pid"`
	StartedAtSecs  uint64  `json:"started_at_secs"`
	Name           string  `json:"name"`
	CpuBasisPoints *uint32 `json:"cpu_basis_points"`
	MemoryBytes    uint64  `json:"memory_bytes"`
}

func DefaultHostMetricsProcess() HostMetricsProcess {
	return HostMetricsProcess{
		Pid:            0,
		StartedAtSecs:  0,
		Name:           "",
		CpuBasisPoints: nil,
		MemoryBytes:    0,
	}
}
func (value *HostMetricsProcess) UnmarshalJSON(data []byte) error {
	type plain HostMetricsProcess
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"pid", "started_at_secs", "name", "memory_bytes"}, false); err != nil {
		return err
	}
	*value = HostMetricsProcess(decoded)
	return nil
}
func (value HostMetricsProcess) MarshalJSON() ([]byte, error) {
	type plain HostMetricsProcess
	encoded := plain(value)
	return json.Marshal(encoded)
}
