package mobile

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"io"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestMobileInvokeOps(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		method string
		input  string
		ok     bool
	}{
		{"new", `{"kind":"digest"}`, true},
		{"new", `{"kind":"speech_input"}`, true},
		{"missing", `{}`, false},
		{"new", `{`, false},
	} {
		t.Run(test.method+test.input, func(t *testing.T) {
			results := make(chan envelope, 2)
			runtime := New(func(_ uint64, data []byte) {
				var value envelope
				if err := json.Unmarshal(data, &value); err != nil {
					t.Errorf("decode completion: %v", err)
				}
				results <- value
			})
			if !runtime.Invoke(0, test.method, []byte(test.input), 1) {
				t.Fatal("operation was not accepted")
			}
			got := <-results
			if got.OK != test.ok {
				t.Fatalf("completion = %#v, want ok=%v", got, test.ok)
			}
			runtime.Close(2)
			if final := <-results; !final.Closed || final.Count != "2" {
				t.Fatalf("close acknowledgement = %#v", final)
			}
		})
	}
}

type waitingObject struct {
	started chan struct{}
	stopped chan struct{}
	once    sync.Once
}

func (owner *waitingObject) invoke(ctx context.Context, _ *Runtime, _ uint64, _ string, _ json.RawMessage) (any, error) {
	owner.once.Do(func() { close(owner.started) })
	<-ctx.Done()
	return nil, ctx.Err()
}
func (owner *waitingObject) close() error {
	close(owner.stopped)
	return nil
}

func TestMobileRootHandleIsNotReleased(t *testing.T) {
	t.Parallel()
	for _, method := range []string{"close", "dispose"} {
		t.Run(method, func(t *testing.T) {
			results := make(chan envelope, 2)
			runtime := New(func(_ uint64, data []byte) {
				var value envelope
				if err := json.Unmarshal(data, &value); err != nil {
					t.Errorf("decode completion: %v", err)
				}
				results <- value
			})
			owner := &waitingObject{started: make(chan struct{}), stopped: make(chan struct{})}
			lifetime, cancel := context.WithCancel(runtime.ctx)
			if _, err := runtime.add(0, "updates", owner, lifetime, cancel); err != nil {
				t.Fatal(err)
			}
			// A handle that was never set is 0 on the Dart side.
			if !runtime.Invoke(0, method, []byte("{}"), 1) {
				t.Fatal("operation was not accepted")
			}
			if got := <-results; got.OK || got.Error == nil || got.Error.Code != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("completion = %#v", got)
			}
			select {
			case <-owner.stopped:
				t.Fatal("releasing the root released another handle")
			default:
			}
			if lifetime.Err() != nil {
				t.Fatal("releasing the root cancelled another handle")
			}
			runtime.Close(2)
			<-owner.stopped
			<-results
		})
	}
}

func TestMobileTransferBytes(t *testing.T) {
	t.Parallel()
	content := []byte{0, 1, 254, 255}
	const encoded = `"AAH+/w=="`
	t.Run("digest", func(t *testing.T) {
		owner := newDigest()
		// Base64 is the bridge's form; a number array is still read.
		for _, input := range []string{`{"bytes":` + encoded + `}`, `{"bytes":[0,1,254,255]}`} {
			if _, err := owner.invoke(t.Context(), nil, 0, "update", json.RawMessage(input)); err != nil {
				t.Fatal(err)
			}
		}
		revision, err := owner.invoke(t.Context(), nil, 0, "revision", nil)
		want := blake3.Sum256(append(append([]byte{}, content...), content...))
		if err != nil || revision != hex.EncodeToString(want[:]) {
			t.Fatalf("revision = %v, %v", revision, err)
		}
	})
	t.Run("upload", func(t *testing.T) {
		reader, writer := io.Pipe()
		owner := &upload{transfer: &transfer{reader: reader, writer: writer}}
		received := make(chan []byte, 1)
		go func() {
			data := make([]byte, 16)
			count, _ := reader.Read(data)
			received <- data[:count]
		}()
		if _, err := owner.invoke(t.Context(), nil, 0, "write", json.RawMessage(`{"bytes":`+encoded+`}`)); err != nil {
			t.Fatal(err)
		}
		if got := <-received; !bytes.Equal(got, content) {
			t.Fatalf("uploaded %v", got)
		}
	})
	t.Run("download", func(t *testing.T) {
		reader, writer := io.Pipe()
		owner := &download{transfer: &transfer{reader: reader, writer: writer}}
		go func() { _, _ = writer.Write(content) }()
		value, err := owner.invoke(t.Context(), nil, 0, "next", nil)
		if err != nil {
			t.Fatal(err)
		}
		completion, err := json.Marshal(result(value, nil))
		if err != nil || string(completion) != `{"ok":true,"value":`+encoded+`}` {
			t.Fatalf("completion = %s, %v", completion, err)
		}
	})
}

func TestMobileSubscribeCancel(t *testing.T) {
	t.Parallel()
	var mu sync.Mutex
	completed := map[uint64]int{}
	callbacks := make(chan uint64, 4)
	runtime := New(func(request uint64, _ []byte) {
		mu.Lock()
		completed[request]++
		mu.Unlock()
		callbacks <- request
	})
	owner := &waitingObject{started: make(chan struct{}), stopped: make(chan struct{})}
	lifetime, cancel := context.WithCancel(runtime.ctx)
	handle, err := runtime.add(0, "updates", owner, lifetime, cancel)
	if err != nil || handle.Handle != "1" {
		t.Fatalf("register handle = %#v, %v", handle, err)
	}
	if !runtime.Invoke(1, "next", []byte("{}"), 1) {
		t.Fatal("subscription was not accepted")
	}
	<-owner.started
	runtime.Cancel(1)
	if request := <-callbacks; request != 1 {
		t.Fatalf("cancel completion = %d", request)
	}
	if !runtime.Invoke(1, "dispose", []byte("{}"), 2) {
		t.Fatal("dispose was not accepted")
	}
	if request := <-callbacks; request != 2 {
		t.Fatalf("dispose completion = %d", request)
	}
	<-owner.stopped
	runtime.Close(3)
	if request := <-callbacks; request != 3 {
		t.Fatalf("close acknowledgement = %d", request)
	}
	mu.Lock()
	defer mu.Unlock()
	for _, request := range []uint64{1, 2, 3} {
		if completed[request] != 1 {
			t.Fatalf("request %d completed %d times", request, completed[request])
		}
	}
}
