package process

import (
	"bytes"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestServiceScannerPartialSchemes(t *testing.T) {
	input := []byte("httph ttthtt\x1b[32mp://localhost:1234/path\x1b[0m\u2003https:hthttps://127.0.0.1:5678/\nhttps://example.org:1234/ HTTP://localhost:9000/\n")
	want := []protocol.ProcessService{{Port: 1234, Url: "http://127.0.0.1:1234/path"}, {Port: 5678, Url: "https://127.0.0.1:5678/"}}
	for split := range len(input) + 1 {
		var scanner serviceScanner
		scanner.append(input[:split])
		scanner.append(input[split:])
		if got := scanner.found; !slices.Equal(got, want) {
			t.Fatalf("split %d: services = %#v, want %#v", split, got, want)
		}
	}
}

func BenchmarkCommandOutputServiceScan(b *testing.B) {
	chunk := bytes.Repeat([]byte("checking package and handling fixture output\n"), 350)
	b.SetBytes(int64(len(chunk)))
	b.ReportAllocs()
	var scanner serviceScanner
	for b.Loop() {
		scanner.append(chunk)
	}
}
