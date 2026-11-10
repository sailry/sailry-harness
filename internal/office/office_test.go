package office

import (
	"archive/zip"
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPositionedOfficeExtraction(t *testing.T) {
	t.Parallel()
	for _, fixture := range []struct {
		name, want, position string
		parts                int
	}{{"report.docx", "中文内容", "section", 1}, {"report.pdf", "PDF acceptance", "page", 2}, {"book.xlsx", "20", "cells", 2}, {"chart.xlsx", "B4: 20 [=SUM(B2:B3)]", "cells", 2}, {"deck.pptx", "中文幻灯片", "slide", 4}} {
		t.Run(fixture.name, func(t *testing.T) {
			t.Parallel()
			data, err := os.ReadFile(filepath.Join("testdata", fixture.name))
			if err != nil {
				t.Fatal(err)
			}
			parts, err := Parts(fixture.name, data)
			if err != nil {
				t.Fatal(err)
			}
			var text strings.Builder
			for _, part := range parts {
				if part.Position.Kind != fixture.position {
					t.Fatal("missing extraction position")
				}
				text.WriteString(part.Text)
			}
			if len(parts) != fixture.parts || !strings.Contains(text.String(), fixture.want) {
				t.Fatalf("parts %d text %q", len(parts), text.String())
			}
			page, err := Inspect(fixture.name, data, 0)
			if err != nil || len(page.Sections) != fixture.parts || len(page.Revision) != 64 {
				t.Fatalf("inspection: %+v %v", page, err)
			}
		})
	}
	t.Run("section chunk pagination", func(t *testing.T) {
		var buffer bytes.Buffer
		writer := zip.NewWriter(&buffer)
		part, err := writer.Create("word/document.xml")
		if err != nil {
			t.Fatal(err)
		}
		text := strings.Repeat("Text ", TextLimit*9/5+1)
		if _, err = part.Write([]byte("<document><p><t>" + text + "</t></p></document>")); err != nil {
			t.Fatal(err)
		}
		if err = writer.Close(); err != nil {
			t.Fatal(err)
		}
		page, err := Inspect("long.docx", buffer.Bytes(), 0)
		if err != nil || len(page.Sections) != 8 || page.Next == nil || *page.Next != 8 {
			t.Fatalf("long first page: %+v %v", page, err)
		}
		for _, section := range page.Sections {
			if len(section.Text) != TextLimit || !section.Truncated {
				t.Fatalf("invalid section chunk: %d %v", len(section.Text), section.Truncated)
			}
		}
		last, err := Inspect("long.docx", buffer.Bytes(), *page.Next)
		if err != nil || len(last.Sections) != 2 || last.Next != nil {
			t.Fatalf("long continuation: %+v %v", last, err)
		}
	})

	t.Run("malformed package bounds", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join("testdata", "invalid-row.xlsx"))
		if err != nil {
			t.Fatal(err)
		}
		if _, err = Parts("invalid-row.xlsx", data); err == nil {
			t.Fatal("invalid worksheet coordinates were accepted")
		}
		var buffer bytes.Buffer
		writer := zip.NewWriter(&buffer)
		for i := 0; i < 2; i++ {
			part, err := writer.Create("word/document.xml")
			if err != nil {
				t.Fatal(err)
			}
			_, _ = part.Write([]byte("<p/>"))
		}
		if err = writer.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err = Parts("duplicate.docx", buffer.Bytes()); err == nil {
			t.Fatal("duplicate document parts were accepted")
		}
	})
}
