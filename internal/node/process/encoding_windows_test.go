package process

import (
	"bytes"
	"strings"
	"testing"
)

func TestNativeOutputEncoding(t *testing.T) {
	t.Run("recent UTF16 boundaries survive exhausted retention", func(t *testing.T) {
		stream := newRetained(t.TempDir(), &outputBudget{}, true, true)
		defer stream.evict()
		value := append([]byte{0xff, 0xfe}, bytes.Repeat([]byte{'a', 0, 0x3d, 0xd8, 0x42, 0xde}, 12000)...)
		stream.append(value)
		stream.finish()
		preview := stream.preview()
		if preview["retained_bytes"] != 0 || preview["transcoded"] != true || preview["invalid_utf8"] != false || strings.Contains(preview["tail"].(string), "\ufffd") || !strings.HasSuffix(preview["tail"].(string), "a🙂") {
			t.Fatal("exhausted retention split recent encoded output", preview)
		}
	})
	for _, fixture := range []struct {
		name  string
		bytes []byte
		want  string
	}{{"UTF-16", []byte{0xff, 0xfe, 'a', 0, 0x2d, 0x4e}, "a中"}, {"OEM", []byte{'a', 0x82}, "aé"}, {"UTF-8 BOM", []byte{0xef, 0xbb, 0xbf, 'a'}, "a"}} {
		t.Run(fixture.name, func(t *testing.T) {
			stream := newRetained(t.TempDir(), &outputBudget{}, false, true)
			defer stream.evict()
			stream.capture.decoder.pageCode = newCodePage(437)
			for _, value := range fixture.bytes {
				stream.append([]byte{value})
			}
			stream.finish()
			capture := stream.snapshot(false)
			if capture.Text != fixture.want || capture.InvalidUtf8 {
				t.Fatal(capture)
			}
			preview := stream.preview()
			if preview["transcoded"] != true || preview["total_bytes"] != uint64(len(fixture.bytes)) || len(preview) != 2 {
				t.Fatal("small transcoded output lost original byte accounting", preview)
			}
			if fixture.name == "UTF-16" {
				if _, err := stream.capture.decoder.page(fixture.bytes[3:], 3, 4, true); err == nil {
					t.Fatal("split UTF-16 offset accepted")
				}
			}
		})
	}
}
