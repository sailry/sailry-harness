package store

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"lukechampine.com/blake3"
)

// CaptureCanonicalPayload externalizes only declared MIME-tagged binary fields.
func CaptureCanonicalPayload(ctx context.Context, db *sql.Conn, data json.RawMessage) (canonical.Payload, error) {
	value := agentJSON(data)
	assets := []canonical.BinarySlot{}
	var capture func(any, string) error
	capture = func(value any, pointer string) error {
		switch x := value.(type) {
		case map[string]any:
			var mime *string
			for _, key := range []string{"mime_type", "mimeType", "mime"} {
				if raw, ok := x[key]; ok {
					mime = agentString(raw)
					break
				}
			}
			if mime != nil {
				for _, field := range []string{"data", "blob", "bytes"} {
					raw, ok := x[field]
					if !ok {
						continue
					}
					var data []byte
					encoding := canonical.Bytes
					valid := false
					switch binary := raw.(type) {
					case string:
						decoded, e := base64.StdEncoding.DecodeString(binary)
						if e == nil && base64.StdEncoding.EncodeToString(decoded) == binary {
							data = decoded
							encoding = canonical.Base64
							valid = true
						}
					case []any:
						data = make([]byte, 0, len(binary))
						valid = true
						for _, v := range binary {
							number, ok := v.(json.Number)
							if !ok {
								valid = false
								break
							}
							n, e := strconv.ParseUint(string(number), 10, 8)
							if e != nil {
								valid = false
								break
							}
							data = append(data, byte(n))
						}
					}
					if !valid {
						continue
					}
					digest, e := PutBlob(ctx, db, data)
					if e != nil {
						return e
					}
					assets = append(assets, canonical.BinarySlot{Pointer: pointer + "/" + field, Asset: canonical.AssetRef{Digest: digest, Mime: *mime, Size: uint64(len(data))}, Encoding: encoding})
					x[field] = nil
				}
			}
			keys := []string{}
			for key := range x {
				keys = append(keys, key)
			}
			slices.Sort(keys)
			for _, key := range keys {
				part := strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
				if e := capture(x[key], pointer+"/"+part); e != nil {
					return e
				}
			}
		case []any:
			for i, v := range x {
				if e := capture(v, fmt.Sprintf("%s/%d", pointer, i)); e != nil {
					return e
				}
			}
		}
		return nil
	}
	if err := capture(value, ""); err != nil {
		return canonical.Payload{}, err
	}
	body, err := encode(value)
	return canonical.Payload{Value: body, Assets: assets}, err
}
func validateCanonicalAssets(ctx context.Context, db *sql.Conn, record canonical.Record) error {
	message := record.MutableMessage()
	if message == nil {
		return nil
	}
	check := func(asset canonical.AssetRef) error {
		data, err := GetBlob(ctx, db, asset.Digest)
		if err != nil {
			return err
		}
		sum := blake3.Sum256(data)
		if uint64(len(data)) != asset.Size || hex.EncodeToString(sum[:]) != asset.Digest {
			return invalid("conversation asset is invalid")
		}
		return nil
	}
	for _, block := range message.Content {
		switch value := block.Data.(type) {
		case *canonical.AssetContent:
			if err := check(value.Asset); err != nil {
				return err
			}
		case *canonical.ToolResponse:
			for _, attachment := range value.Attachments {
				if err := check(attachment.Asset); err != nil {
					return err
				}
			}
			for _, slot := range value.Result.Assets {
				if agentPointer(agentJSON(value.Result.Value), slot.Pointer) != nil {
					return invalid("conversation asset slot is invalid")
				}
				if err := check(slot.Asset); err != nil {
					return err
				}
			}
			hydrated, err := HydrateCanonicalPayload(ctx, db, value.Result)
			if err != nil {
				return err
			}
			body, err := encode(agentJSON(hydrated))
			if err != nil {
				return err
			}
			sum := blake3.Sum256(body)
			if hex.EncodeToString(sum[:]) != value.Revision {
				return invalid("tool output revision is invalid")
			}
		}
	}
	return nil
}
