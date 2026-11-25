package logging

import (
	"encoding/json"
	"io"
	"log/slog"
	"os"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/Masterminds/semver/v3"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// File, remote and diagnostic records share this single field policy. Local
// identifiers remain useful on disk but never leave the process.
type disclosure int

const (
	local disclosure = iota
	public
	version
)

var fields = []struct {
	name       string
	disclosure disclosure
}{
	{"area", public}, {"phase", public}, {"node", local}, {"caller", local}, {"request", local}, {"incident", local},
	{"plugin_version", local}, {"app_version", version}, {"platform", local}, {"exception", local}, {"session", local},
	{"turn", local}, {"worktree", local}, {"provider", local}, {"provider_name", local}, {"model", local},
	{"package", local}, {"digest", local}, {"settings_revision", local}, {"handler", local}, {"invocation", local},
	{"call", local}, {"artifact_id", local}, {"attachment_id", local}, {"revision", local}, {"command", local},
	{"operation", public}, {"stage", local}, {"detail", local}, {"source_kind", local}, {"reason", local},
	{"command_stage", local}, {"configuration_reason", local}, {"reason_code", local}, {"report_ref", local},
	{"cause_code", public}, {"cause_component", local}, {"cause_category", local}, {"upstream_status", local},
	{"websocket_close_code", local}, {"attempt", local}, {"limit", local}, {"request_mode", local}, {"transport", local},
	{"connection_reused", local}, {"communication_outcome", local}, {"execution_outcome", local},
	{"recovery_decision", local}, {"wait_ms", local}, {"end_to_end_us", local}, {"rtt_ms", local}, {"generation", local},
	{"sequence", local}, {"fallback_reason", local}, {"continuation", local}, {"stream", local}, {"budget", local},
	{"parse_layer", local}, {"parse_category", local}, {"parse_cause", local}, {"parse_field", local},
	{"parse_event", local}, {"parse_line", local}, {"parse_column", local}, {"code", public}, {"status", local},
	{"duration_us", public}, {"first_token_us", local}, {"bytes", local}, {"count", public}, {"durable", local},
	{"elapsed_us", public}, {"last_content_us", local},
	{"retry_outcome", local}, {"attempts", local}, {"http_status", local},
}

var known = func() map[string]bool {
	names := map[string]bool{}
	for _, field := range fields {
		names[field.name] = true
	}
	return names
}()

// Fields are a record's allowlisted values: strings, int64, uint64 and bool.
type Fields map[string]any

// collect keeps the allowlisted attributes. Strings lose control characters
// and everything after 256 characters; an empty string is no value.
func collect(record slog.Record) Fields {
	out := Fields{}
	record.Attrs(func(attr slog.Attr) bool {
		if !known[attr.Key] {
			return true
		}
		switch value := attr.Value.Resolve(); value.Kind() {
		case slog.KindString:
			text := []rune(strings.Map(func(r rune) rune {
				if unicode.IsControl(r) {
					return -1
				}
				return r
			}, value.String()))
			if len(text) > 256 {
				text = text[:256]
			}
			if len(text) > 0 {
				out[attr.Key] = string(text)
			}
		case slog.KindInt64:
			out[attr.Key] = value.Int64()
		case slog.KindUint64:
			out[attr.Key] = value.Uint64()
		case slog.KindBool:
			out[attr.Key] = value.Bool()
		}
		return true
	})
	return out
}

func levelName(level slog.Level) string {
	switch {
	case level >= slog.LevelError:
		return "ERROR"
	case level >= slog.LevelWarn:
		return "WARN"
	case level >= slog.LevelInfo:
		return "INFO"
	}
	return "DEBUG"
}

// line is one record as the file keeps it: time, level, application and the
// fields in the policy's order.
func line(at time.Time, level slog.Level, application string, values Fields) string {
	var text strings.Builder
	text.WriteString(at.UTC().Format("2006-01-02T15:04:05.000000Z"))
	text.WriteByte(' ')
	text.WriteString(levelName(level))
	text.WriteByte(' ')
	text.WriteString(application)
	for _, field := range fields {
		value, found := values[field.name]
		if !found {
			continue
		}
		text.WriteByte(' ')
		text.WriteString(field.name)
		text.WriteByte('=')
		switch value := value.(type) {
		case string:
			if bare(value) {
				text.WriteString(value)
			} else {
				encoded, _ := json.Marshal(value)
				text.Write(encoded)
			}
		case int64:
			text.WriteString(strconv.FormatInt(value, 10))
		case uint64:
			text.WriteString(strconv.FormatUint(value, 10))
		case bool:
			text.WriteString(strconv.FormatBool(value))
		}
	}
	text.WriteByte('\n')
	return text.String()
}

// bare reports whether a string reads back as itself without quotes.
func bare(value string) bool {
	for _, r := range value {
		if unicode.IsSpace(r) || unicode.IsControl(r) || r == '"' || r == '\\' || r == '=' {
			return false
		}
	}
	return !json.Valid([]byte(value))
}

func token(value string) bool {
	if value == "" || len(value) > 64 || value[0] < 'a' || value[0] > 'z' || strings.Contains(value, "..") {
		return false
	}
	for _, b := range []byte(value) {
		if !(b >= 'a' && b <= 'z' || b >= '0' && b <= '9' || b == '_' || b == '.') {
			return false
		}
	}
	return true
}

// ValidReportRef accepts only canonical, random v4 references. In particular
// a request's timestamped v7 identifier is not a report reference.
func ValidReportRef(s string) bool {
	if len(s) != 36 {
		return false
	}
	id, err := uuid.Parse(s)
	return err == nil && id.Version() == 4 && id.Variant() == uuid.RFC4122 && id.String() == s
}

// Remote is the part of a record that may leave the process: public tokens
// and counters, a valid application version, and a package only as
// "third_party". Diagnostic stages and reasons use closed lists.
// Identifiers, names, details and paths never pass.
func Remote(values Fields) Fields {
	safe := Fields{}
	for _, field := range fields {
		if field.disclosure == local {
			continue
		}
		switch value := values[field.name].(type) {
		case string:
			if field.disclosure == version {
				if _, err := semver.StrictNewVersion(value); err != nil || len(value) > 64 {
					continue
				}
			} else if !token(value) {
				continue
			}
			safe[field.name] = value
		case int64, uint64, bool:
			if field.disclosure == public {
				safe[field.name] = value
			}
		}
	}
	if ref, _ := values["report_ref"].(string); safe["phase"] == Failed && ValidReportRef(ref) {
		safe["report_ref"] = ref
	}
	if safe["phase"] == Failed {
		switch reason, _ := values["reason_code"].(string); reason {
		case "draft_not_saveable", "selection_unmapped", "queue_revision_changed", "queue_item_not_pending", "queue_input_consumed", "queue_input_not_editable", "guidance_not_pending", "recipient_stopping", "recipient_not_running", "recipient_not_task", "recipient_is_source", "guidance_scope_mismatch":
			safe["reason_code"] = reason
		}
		stage, _ := values["stage"].(string)
		if (safe["area"] == "desktop" || safe["area"] == "command") && safe["operation"] == "inspect_git" {
			reason, _ := values["reason"].(string)
			if stage != "" && protocol.GitInspectionReason(reason).Stage() == stage && safe["code"] != string(protocol.ErrorCodeCancelled) {
				safe["stage"], safe["reason"] = stage, reason
			}
		} else if safe["area"] == "desktop" {
			if safe["operation"] == "preferences_save_failed" {
				reason, _ := values["reason"].(string)
				if preferencesWriteDiagnostic(stage, reason) && safe["code"] != string(protocol.ErrorCodeCancelled) {
					safe["stage"], safe["reason"] = stage, reason
				}
			} else {
				switch stage {
				case "preflight", "map_selection", "workspace_mutation", "file_search":
					safe["stage"] = stage
				}
			}
		}
		if safe["area"] == "command" && (safe["operation"] == "check_ssh" || safe["operation"] == "install_host") {
			stage, _ := values["stage"].(string)
			reason, _ := values["reason"].(string)
			if stage != "" && protocol.SSHFailureReason(reason).Stage() == stage && safe["code"] != string(protocol.ErrorCodeCancelled) {
				safe["stage"], safe["reason"] = stage, reason
			}
		}
	}
	if name, _ := values["package"].(string); name != "" {
		safe["package"] = "third_party"
	}
	if reason, _ := values["configuration_reason"].(string); protocol.ConfigurationReason(reason).Known() {
		safe["configuration_reason"] = reason
	}
	if stage, _ := values["command_stage"].(string); stage == "admission" || stage == "completion" {
		safe["command_stage"] = stage
	}
	if safe["area"] == "agent" && safe["operation"] == "model_request" {
		outcome, _ := values["retry_outcome"].(string)
		if outcome == "failed" && safe["phase"] == Failed || outcome == "recovered" && safe["phase"] == Completed {
			switch reason, _ := values["reason_code"].(string); reason {
			case "model_not_configured", "model_authentication", "model_context_overflow", "model_rate_limited", "model_quota", "model_network", "model_unavailable", "model_invalid_request", "model_invalid_response", "model_unsupported", "model_other":
				if attempts, ok := boundedInteger(values["attempts"], 1, 1_000_000); ok && (outcome != "recovered" || attempts > 1) {
					safe["retry_outcome"], safe["reason_code"], safe["attempts"] = outcome, reason, attempts
					if status, ok := boundedInteger(values["http_status"], 100, 599); ok {
						safe["http_status"] = status
					}
				}
			}
		}
	}
	return safe
}

func boundedInteger(value any, minimum, maximum uint64) (uint64, bool) {
	var number uint64
	switch value := value.(type) {
	case int64:
		if value < 0 {
			return 0, false
		}
		number = uint64(value)
	case uint64:
		number = value
	default:
		return 0, false
	}
	return number, number >= minimum && number <= maximum
}

func preferencesWriteDiagnostic(stage, reason string) bool {
	switch stage {
	case "create_directory", "create_temp", "write", "sync", "close", "replace":
		switch reason {
		case "permission_denied", "not_found", "already_exists", "no_space", "file_locked", "other":
			return true
		}
	}
	return false
}

const (
	tailLines  = 500
	tailWindow = 1024 * 1024
)

// Tail is the diagnostics attachment: the last records of the active file's
// final mebibyte, each reduced by Remote, one JSON object per line. An empty
// path gives an empty attachment.
func Tail(path string) ([]byte, error) {
	if path == "" {
		return []byte{}, nil
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	start := max(info.Size()-tailWindow, 0)
	if _, err = file.Seek(start, io.SeekStart); err != nil {
		return nil, err
	}
	data, err := io.ReadAll(io.LimitReader(file, tailWindow))
	if err != nil {
		return nil, err
	}
	text := strings.ToValidUTF8(string(data), "\uFFFD")
	if start > 0 {
		_, text, _ = strings.Cut(text, "\n")
	}
	lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	if len(lines) > tailLines {
		lines = lines[len(lines)-tailLines:]
	}
	out := []byte{}
	for _, entry := range lines {
		if values, valid := parse(strings.TrimSuffix(entry, "\r")); valid {
			encoded, err := json.Marshal(values)
			if err != nil {
				continue
			}
			out = append(append(out, encoded...), '\n')
		}
	}
	return out, nil
}

// parse reads a line this package wrote. Anything else is not a record.
func parse(entry string) (Fields, bool) {
	header := strings.SplitN(entry, " ", 4)
	if len(header) < 3 {
		return nil, false
	}
	if _, err := time.Parse(time.RFC3339Nano, header[0]); err != nil {
		return nil, false
	}
	level := header[1]
	if level != "ERROR" && level != "WARN" && level != "INFO" && level != "DEBUG" {
		return nil, false
	}
	if header[2] != "host" && header[2] != "desktop" {
		return nil, false
	}
	rest := ""
	if len(header) == 4 {
		rest = header[3]
	}
	values := Fields{}
	for rest != "" {
		key, tail, found := strings.Cut(rest, "=")
		if !found || key == "" || strings.ContainsFunc(key, func(r rune) bool { return !(r >= 'a' && r <= 'z' || r == '_') }) {
			return nil, false
		}
		var value any
		used := 0
		if strings.HasPrefix(tail, `"`) {
			decoder := json.NewDecoder(strings.NewReader(tail))
			if decoder.Decode(&value) != nil {
				return nil, false
			}
			used = int(decoder.InputOffset())
		} else {
			used = strings.IndexByte(tail, ' ')
			if used < 0 {
				used = len(tail)
			}
			value = scalar(tail[:used])
		}
		if _, repeated := values[key]; repeated {
			return nil, false
		}
		values[key] = value
		rest = tail[used:]
		if rest != "" {
			if !strings.HasPrefix(rest, " ") {
				return nil, false
			}
			rest = rest[1:]
		}
	}
	values = Remote(values)
	// Manual log tails do not link other deliveries to this diagnostic.
	delete(values, "report_ref")
	values["level"], values["timestamp"] = level, header[0]
	return values, true
}

// scalar is an unquoted value: an integer, a boolean, or the text itself.
func scalar(text string) any {
	if value, err := strconv.ParseUint(text, 10, 64); err == nil {
		return value
	}
	if value, err := strconv.ParseInt(text, 10, 64); err == nil {
		return value
	}
	if text == "true" || text == "false" {
		return text == "true"
	}
	if json.Valid([]byte(text)) {
		// Floats, null and structures are not values this package writes.
		return nil
	}
	return text
}
