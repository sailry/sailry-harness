package mcp

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type inputFixtureArgs struct {
	Name string `json:"name"`
}

func inputFixture(t *testing.T, run sdk.ToolHandlerFor[inputFixtureArgs, any]) (*Pool, *Lease) {
	t.Helper()
	pool := New(t.Context(), Options{Open: func(ctx context.Context, _ Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		server := sdk.NewServer(&sdk.Implementation{Name: "input-fixture", Version: "1"}, nil)
		sdk.AddTool(server, &sdk.Tool{Name: "read", Annotations: &sdk.ToolAnnotations{ReadOnlyHint: true}}, run)
		clientWire, serverWire := sdk.NewInMemoryTransports()
		session, err := server.Connect(t.Context(), serverWire, nil)
		if err != nil {
			return nil, err
		}
		t.Cleanup(func() { _ = session.Close() })
		client := sdk.NewClient(&sdk.Implementation{Name: "input-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit})
		return client.Connect(ctx, clientWire, &sdk.ClientSessionOptions{ProtocolVersion: "2026-07-28"})
	}})
	t.Cleanup(func() { _ = pool.Close() })
	lease, err := pool.Acquire(t.Context(), Configuration{
		Package: protocol.PluginReference{Name: "inputs", Digest: "fixture"}, Server: "server", Options: protocol.DefaultPluginMcpOptions(),
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = lease.Close() })
	return pool, lease
}

func inputFixtureResult(ctx context.Context, request *sdk.CallToolRequest, input inputFixtureArgs) (*sdk.CallToolResult, any, error) {
	if input.Name == "slow" {
		<-ctx.Done()
		return nil, nil, ctx.Err()
	}
	if len(request.Params.InputResponses) == 0 {
		return &sdk.CallToolResult{
			InputRequests: sdk.InputRequestMap{"answer": &sdk.ElicitParams{Message: input.Name}},
			RequestState:  input.Name,
		}, nil, nil
	}
	answer, ok := request.Params.InputResponses["answer"].(*sdk.ElicitResult)
	if !ok || answer.Content["answer"] != input.Name || request.Params.RequestState != input.Name {
		return nil, nil, fmt.Errorf("input response belongs to a different tool call: %s", input.Name)
	}
	return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: input.Name}}}, nil, nil
}

func TestMCPConcurrentInputs(t *testing.T) {
	t.Parallel()
	for _, cancelFirst := range []bool{false, true} {
		t.Run(fmt.Sprintf("cancel=%t", cancelFirst), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				pool, lease := inputFixture(t, inputFixtureResult)
				first, cancel := context.WithCancel(t.Context())
				defer cancel()
				var contexts sync.Map
				entered := make(chan string, 2)
				answers := []chan struct{}{make(chan struct{}), make(chan struct{})}
				results := []chan error{make(chan error, 1), make(chan error, 1)}
				for i, name := range []string{"first", "second"} {
					go func() {
						ctx := t.Context()
						if i == 0 {
							ctx = first
						}
						result, err := lease.Call(ctx, "read", map[string]any{"name": name}, true, func(ctx context.Context, request *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
							if request.Params.Message != name {
								return nil, fmt.Errorf("%s received %s's question", name, request.Params.Message)
							}
							contexts.Store(name, ctx)
							entered <- name
							select {
							case <-answers[i]:
								return &sdk.ElicitResult{Action: "accept", Content: map[string]any{"answer": name}}, nil
							case <-ctx.Done():
								return nil, ctx.Err()
							}
						})
						if err == nil && (len(result.Content) != 1 || result.Content[0].(*sdk.TextContent).Text != name) {
							err = fmt.Errorf("%s received another call's result: %+v", name, result)
						}
						results[i] <- err
					}()
				}
				synctest.Wait()
				if len(entered) != 2 {
					t.Fatalf("only %d input requests ran together", len(entered))
				}
				// Even with active calls, an unrelated reverse request must not pick one.
				if _, err := lease.connection.elicit(t.Context(), &sdk.ElicitRequest{Params: &sdk.ElicitParams{Message: "unrelated"}}); err == nil {
					t.Fatal("unrelated input was assigned to an active call")
				}
				config := lease.config
				config.Server = "other-server"
				other, err := pool.Acquire(t.Context(), config)
				if err != nil {
					t.Fatal(err)
				}
				defer other.Close()
				captured, _ := contexts.Load("first")
				if _, err := other.connection.elicit(captured.(context.Context), &sdk.ElicitRequest{}); err == nil {
					t.Fatal("another connection accepted the call's input context")
				}
				finish := []int{1, 0}
				if cancelFirst {
					cancel()
					var fault *protocol.Fault
					if err := <-results[0]; !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeCancelled {
						t.Fatalf("cancelled input call = %v", err)
					}
					if captured.(context.Context).Err() == nil {
						t.Fatal("cancelled call left its input handler running")
					}
					neighbor, _ := contexts.Load("second")
					if neighbor.(context.Context).Err() != nil {
						t.Fatal("cancelling one input cancelled the other")
					}
					finish = []int{1}
				}
				// Complete out of order to exercise response routing independently.
				for _, i := range finish {
					close(answers[i])
					if err := <-results[i]; err != nil {
						t.Fatal(err)
					}
				}
				if _, err := lease.connection.elicit(captured.(context.Context), &sdk.ElicitRequest{}); err == nil {
					t.Fatal("completed call accepted a late input request")
				}
				reused, err := pool.Acquire(t.Context(), lease.config)
				if err != nil {
					t.Fatal(err)
				}
				defer reused.Close()
				if reused.connection != lease.connection {
					t.Fatal("input handling retired the shared connection")
				}
			})
		})
	}
}

func TestMCPInputWaitPausesOnlyItsCall(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		_, lease := inputFixture(t, inputFixtureResult)
		answer := make(chan struct{})
		asked := make(chan struct{})
		waiting, slow := make(chan error, 1), make(chan error, 1)
		go func() {
			_, err := lease.Call(t.Context(), "read", map[string]any{"name": "question"}, true, func(ctx context.Context, _ *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
				close(asked)
				select {
				case <-answer:
					return &sdk.ElicitResult{Action: "accept", Content: map[string]any{"answer": "question"}}, nil
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			})
			waiting <- err
		}()
		go func() {
			_, err := lease.Call(t.Context(), "read", map[string]any{"name": "slow"}, true, nil)
			slow <- err
		}()
		<-asked
		synctest.Wait()
		time.Sleep(121 * time.Second)
		synctest.Wait()
		select {
		case err := <-slow:
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("unanswered read timeout = %v", err)
			}
		default:
			t.Fatal("input wait paused another call's timeout")
		}
		select {
		case err := <-waiting:
			t.Fatalf("user input consumed the call deadline: %v", err)
		default:
		}
		close(answer)
		if err := <-waiting; err != nil {
			t.Fatal(err)
		}
	})
}

func TestMCPReadInputFailureKeepsConnection(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		pool, lease := inputFixture(t, inputFixtureResult)
		denied := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "input is not permitted"}
		_, err := lease.Call(t.Context(), "read", map[string]any{"name": "denied"}, true, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
			return nil, denied
		})
		if !errors.Is(err, denied) {
			t.Fatalf("input rejection lost its cause: %v", err)
		}
		reused, err := pool.Acquire(t.Context(), lease.config)
		if err != nil {
			t.Fatal(err)
		}
		defer reused.Close()
		if reused.connection != lease.connection {
			t.Fatal("input rejection retired the shared connection")
		}
		_, err = reused.Call(t.Context(), "read", map[string]any{"name": "next"}, true, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
			return &sdk.ElicitResult{Action: "accept", Content: map[string]any{"answer": "next"}}, nil
		})
		if err != nil {
			t.Fatalf("input rejection affected the next call: %v", err)
		}
	})
}
