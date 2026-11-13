package acceptance

import (
	"bytes"
	"encoding/json"
	"errors"
	"html/template"
	"os"
	"path/filepath"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
)

type ReportPaths struct {
	JSON string `json:"json"`
	HTML string `json:"html"`
}

// WriteReport publishes private, complete files at fixed paths. The report
// contains test-owned evidence; HTML always treats that evidence as plain text.
func WriteReport(directory string, report Report) (ReportPaths, error) {
	if directory == "" {
		return ReportPaths{}, errors.New("acceptance report directory is required")
	}
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return ReportPaths{}, err
	}
	paths := ReportPaths{JSON: filepath.Join(absolute, "latest.json"), HTML: filepath.Join(absolute, "latest.html")}
	data, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		return ReportPaths{}, err
	}
	var page bytes.Buffer
	if err := reportPage.Execute(&page, report); err != nil {
		return ReportPaths{}, err
	}
	if err := core.PrivateDirectory(absolute); err != nil {
		return ReportPaths{}, err
	}
	for _, file := range []struct {
		path string
		data []byte
	}{{paths.JSON, append(data, '\n')}, {paths.HTML, page.Bytes()}} {
		if err := writeReportFile(file.path, file.data); err != nil {
			return ReportPaths{}, err
		}
	}
	return paths, nil
}

func writeReportFile(path string, data []byte) error {
	file, err := os.CreateTemp(filepath.Dir(path), ".acceptance-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return os.Rename(file.Name(), path)
}

var reportPage = template.Must(template.New("acceptance").Funcs(template.FuncMap{
	"timestamp": func(value time.Time) string { return value.UTC().Format(time.RFC3339) },
	"duration":  func(value time.Duration) string { return value.Round(time.Millisecond).String() },
	"count": func(report Report, status string) int {
		count := 0
		for _, result := range report.Cases {
			if result.Status == status {
				count++
			}
		}
		return count
	},
	"statusClass": func(status string) string {
		switch status {
		case "passed", "failed", "blocked", "canceled", "skipped":
			return status
		default:
			return "unknown"
		}
	},
	"summary": func(report Report) string {
		if report.OK() {
			return "Acceptance passed"
		}
		return "Acceptance needs attention"
	},
}).Parse(`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Sailry conversation acceptance</title>
<style>
:root { color-scheme: light dark; font: 15px/1.55 system-ui, sans-serif; }
body { max-width: 1040px; padding: 32px 24px 64px; margin: auto; }
h1 { font-size: 28px; margin-bottom: 8px; } h2 { font-size: 20px; }
.muted, dt { opacity: .7; } dl { display: grid; grid-template-columns: 100px 1fr; gap: 6px 16px; }
dd { margin: 0; overflow-wrap: anywhere; } .counts { display: flex; flex-wrap: wrap; gap: 8px 20px; padding: 16px 0; }
article { border-top: 1px solid #8886; padding: 20px 0; } article h2 { margin: 0 0 8px; }
.status { display: inline-block; border: 1px solid currentColor; border-radius: 5px; padding: 1px 7px; font-size: 12px; }
.passed { color: #198754; } .failed, .blocked { color: #c54a37; }
.skipped, .canceled, .unknown { opacity: .7; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; padding: 12px; background: #8881; border: 1px solid #8883; border-radius: 6px; }
details { margin-top: 12px; } summary { cursor: pointer; } li { overflow-wrap: anywhere; }
</style>
</head>
<body>
<header><p class="muted">Sailry · Conversation acceptance</p><h1>{{summary .}}</h1>
<p>These results cover only the cases and execution environment recorded below.</p></header>
<dl><dt>Suite</dt><dd>{{.Suite}}</dd><dt>Provider</dt><dd>{{.Provider}}</dd><dt>Model</dt><dd>{{.Model}}</dd>
{{if .Effort}}<dt>Reasoning effort</dt><dd>{{.Effort.Kind}}{{if eq .Effort.Kind "budget"}} · {{.Effort.Data}}{{end}}</dd>{{end}}
<dt>Started</dt><dd>{{timestamp .Started}}</dd><dt>Finished</dt><dd>{{timestamp .Finished}}</dd></dl>
<div class="counts"><span>{{len .Cases}} cases</span><span>{{count . "passed"}} passed</span><span>{{count . "failed"}} failed</span><span>{{count . "blocked"}} blocked</span><span>{{count . "skipped"}} skipped</span><span>{{count . "canceled"}} stopped</span></div>
<main>{{range .Cases}}
<article><h2>{{.ID}} · {{.Title}}</h2><span class="status {{statusClass .Status}}">{{.Status}}</span> <span class="muted">{{.Group}} · {{duration .Duration}}</span>
<p>{{.Detail}}</p>{{if .Session}}<p class="muted">Session: {{.Session}}</p>{{end}}
{{if .Turns}}<details><summary>Turns</summary><ul>{{range .Turns}}<li>{{.}}</li>{{end}}</ul></details>{{end}}
{{if .Evidence}}<details><summary>Evidence</summary>{{range .Evidence}}<p><strong>{{.Kind}}</strong></p><pre>{{.Value}}</pre>{{end}}</details>{{end}}
</article>{{else}}<p>No acceptance cases ran.</p>{{end}}</main>
</body></html>
`))
