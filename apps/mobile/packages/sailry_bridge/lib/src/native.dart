import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'envelope.dart';

NativeBridge? _shared;

Future<void> initializeNativeBridge({DynamicLibrary? library}) async {
  _openNativeBridge(library);
}

NativeBridge get nativeBridge => _openNativeBridge(null);

NativeBridge _openNativeBridge(DynamicLibrary? library) {
  final existing = _shared;
  if (existing != null && !existing._closing) return existing;
  return _shared = NativeBridge(library);
}

class _Release {
  const _Release(this.bridge, this.handle);

  final NativeBridge bridge;
  final int handle;
}

final _finalizer = Finalizer<_Release>(
  (release) => release.bridge.disposeHandle(release.handle),
);

abstract class NativeHandle {
  NativeHandle(this.bridge, this.handle) {
    _finalizer.attach(this, _Release(bridge, handle), detach: this);
  }

  final NativeBridge bridge;
  final int handle;
  bool _disposed = false;

  Future<Object?> invoke(String method, [Map<String, Object?>? arguments]) {
    if (_disposed) return Future.error(StateError('native handle is disposed'));
    return bridge.invoke(handle, method, arguments ?? const {});
  }

  Future<Object?> nextValue() => _nextValue(const {});

  Future<Object?> _nextValue(Map<String, Object?> arguments) {
    if (_disposed) return Future.error(StateError('native handle is disposed'));
    return bridge.invoke(handle, 'next', arguments, subscribe: true);
  }

  Future<void> closeNative() async {
    if (!_disposed) await invoke('close');
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _finalizer.detach(this);
    bridge.disposeHandle(handle);
  }
}

Future<String> nextObservationView(NativeHandle observer) async =>
    await observer._nextValue(const {'raw_view': true}) as String;

int nativeHandle(Object? value, String kind) {
  if (value is! Map<String, dynamic> || value['kind'] != kind) {
    throw const FormatException('native result has an unexpected handle kind');
  }
  return BigInt.parse(value['handle'] as String).toSigned(64).toInt();
}

class NativeBridge {
  NativeBridge(DynamicLibrary? library) {
    bindings = SailryBindings(library ?? _library());
    // Results arrive as port messages rather than native callbacks: once this
    // isolate exits, the native side's posts fail instead of calling code
    // that no longer exists, and it closes the orphaned runtime.
    _results.listen(_receive);
    identifier = bindings.sailry_open(
      NativeApi.initializeApiDLData,
      _results.sendPort.nativePort,
    );
    if (identifier == 0) {
      _results.close();
      throw StateError('native bridge could not be opened');
    }
  }

  late final SailryBindings bindings;
  final _results = ReceivePort('sailry bridge');
  late final int identifier;
  final Map<int, Completer<Object?>> _pending = {};
  final Set<int> _handles = {};
  int _request = 0;
  int _received = 0;
  int? _finalCount;
  bool _closing = false;
  bool _closed = false;
  bool _disposeRequested = false;
  Completer<void>? _shutdown;

  int _nextRequest() {
    if (_request == 0x7fffffffffffffff) {
      throw StateError('native operation identifiers are exhausted');
    }
    return ++_request;
  }

  Future<Object?> invoke(
    int handle,
    String method,
    Map<String, Object?> arguments, {
    bool subscribe = false,
  }) {
    if (_closing) return Future.error(StateError('native bridge is closed'));
    if (handle == 0 && (method == 'open' || method == 'new')) {
      _disposeRequested = true;
    }
    final request = _nextRequest();
    final completer = Completer<Object?>();
    _pending[request] = completer;
    Pointer<Utf8>? operation;
    Pointer<Uint8>? input;
    Pointer<Pointer<Uint8>>? immediate;
    Pointer<Uint64>? immediateLength;
    try {
      final data = utf8.encode(jsonEncode(arguments));
      operation = method.toNativeUtf8();
      input = calloc<Uint8>(data.length);
      input.asTypedList(data.length).setAll(0, data);
      immediate = calloc<Pointer<Uint8>>();
      immediateLength = calloc<Uint64>();
      final accepted = subscribe
          ? bindings.sailry_subscribe(
              identifier,
              handle,
              operation.cast(),
              input,
              data.length,
              request,
            )
          : bindings.sailry_invoke(
              identifier,
              handle,
              operation.cast(),
              input,
              data.length,
              request,
              immediate,
              immediateLength,
            );
      if (immediate.value != nullptr) {
        try {
          _resolve(
            request,
            _decode(immediate.value.asTypedList(immediateLength.value)),
          );
        } finally {
          bindings.sailry_free(immediate.value);
        }
      } else if (accepted == 0) {
        _pending.remove(request);
        completer.completeError(
          StateError('native operation was not accepted'),
        );
      }
    } catch (error, stack) {
      _pending.remove(request);
      if (!completer.isCompleted) completer.completeError(error, stack);
    } finally {
      if (operation != null) calloc.free(operation);
      if (input != null) calloc.free(input);
      if (immediate != null) calloc.free(immediate);
      if (immediateLength != null) calloc.free(immediateLength);
    }
    return completer.future;
  }

  Object? invokeSync(String method, Map<String, Object?> arguments) {
    if (_closing) throw StateError('native bridge is closed');
    Pointer<Utf8>? operation;
    Pointer<Uint8>? input;
    Pointer<Pointer<Uint8>>? immediate;
    Pointer<Uint64>? immediateLength;
    try {
      final data = JsonUtf8Encoder().convert(arguments);
      operation = method.toNativeUtf8();
      input = calloc<Uint8>(data.length);
      input.asTypedList(data.length).setAll(0, data);
      immediate = calloc<Pointer<Uint8>>();
      immediateLength = calloc<Uint64>();
      final accepted = bindings.sailry_invoke(
        identifier,
        0,
        operation.cast(),
        input,
        data.length,
        0,
        immediate,
        immediateLength,
      );
      if (accepted != 0 || immediate.value == nullptr) {
        throw StateError('native synchronous result is unavailable');
      }
      final result = _decode(
        immediate.value.asTypedList(immediateLength.value),
      );
      if (!result.ok) {
        throw result.error ??
            StateError('native operation failed without an error');
      }
      return result.value;
    } finally {
      if (immediate != null && immediate.value != nullptr) {
        bindings.sailry_free(immediate.value);
      }
      if (operation != null) calloc.free(operation);
      if (input != null) calloc.free(input);
      if (immediate != null) calloc.free(immediate);
      if (immediateLength != null) calloc.free(immediateLength);
    }
  }

  void cancel(int request) {
    if (!_closing) bindings.sailry_cancel(identifier, request);
  }

  void disposeHandle(int handle) {
    if (_closing) return;
    _disposeRequested = true;
    unawaited(
      invoke(handle, 'dispose', const {}).then(
        (_) {
          _handles.remove(handle);
          _closeIfUnused();
        },
        onError: (Object error, StackTrace stack) {
          _handles.remove(handle);
          _closeIfUnused();
        },
      ),
    );
  }

  Future<void> shutdown() {
    if (_closing) return _shutdown!.future;
    _closing = true;
    _shutdown = Completer<void>();
    bindings.sailry_close(identifier, _nextRequest());
    return _shutdown!.future;
  }

  void _receive(Object? message) {
    // A null message only probes that this port is alive.
    if (message case [final int request, final Uint8List data]) {
      unawaited(_complete(request, data));
    }
  }

  Future<void> _complete(int request, Uint8List data) async {
    try {
      final result = data.length < 32 * 1024
          ? _decode(data)
          : await _decodeLarge(data);
      if (result.closed) {
        _finalCount = int.parse(result.count!);
      } else {
        _resolve(request, result);
      }
    } catch (error, stack) {
      final waiting = _pending.remove(request);
      if (waiting != null && !waiting.isCompleted) {
        waiting.completeError(error, stack);
      }
    } finally {
      _received++;
      _finishShutdown();
      if (_disposeRequested) _closeIfUnused();
    }
  }

  // Static, so the worker closure captures only the bytes.
  static Future<CompletionEnvelope> _decodeLarge(Uint8List data) =>
      Isolate.run(() => _decode(data));

  static CompletionEnvelope _decode(Uint8List data) {
    // A leading zero byte marks a raw UTF-8 value rather than an envelope.
    if (data.isNotEmpty && data[0] == 0) {
      return CompletionEnvelope(
        ok: true,
        value: utf8.decode(Uint8List.sublistView(data, 1)),
      );
    }
    return CompletionEnvelope.fromJson(
      jsonDecode(utf8.decode(data)) as Map<String, dynamic>,
    );
  }

  void _resolve(int request, CompletionEnvelope result) {
    final waiting = _pending.remove(request);
    if (waiting == null || waiting.isCompleted) return;
    final value = result.value;
    if (result.ok) {
      if (value is Map<String, dynamic> && value['handle'] is String) {
        _handles.add(
          BigInt.parse(value['handle'] as String).toSigned(64).toInt(),
        );
      }
      waiting.complete(value);
    } else {
      waiting.completeError(
        result.error ?? StateError('native operation failed without an error'),
      );
    }
  }

  void _closeIfUnused() {
    if (_handles.isEmpty && _pending.isEmpty && !_closing) {
      unawaited(shutdown());
    }
  }

  void _finishShutdown() {
    if (_closed || _finalCount == null || _received != _finalCount) return;
    _closed = true;
    _results.close();
    for (final waiting in _pending.values) {
      if (!waiting.isCompleted) {
        waiting.completeError(StateError('native bridge is closed'));
      }
    }
    _pending.clear();
    _handles.clear();
    if (identical(_shared, this)) _shared = null;
    _shutdown?.complete();
  }
}

DynamicLibrary _library() {
  final explicit = Platform.environment['SAILRY_MOBILE_LIBRARY'];
  if (explicit != null && explicit.isNotEmpty) {
    return DynamicLibrary.open(explicit);
  }
  if (Platform.isIOS) return DynamicLibrary.process();
  if (Platform.isAndroid || Platform.isLinux) {
    return DynamicLibrary.open('libsailry_mobile.so');
  }
  if (Platform.isMacOS) {
    return DynamicLibrary.open('libsailry_mobile.dylib');
  }
  throw UnsupportedError('Sailry Mobile is unavailable on this platform');
}
