import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'envelope.dart';

typedef _Completion = Void Function(Uint64, Pointer<Uint8>, Uint64);

NativeBridge? _shared;
Future<void>? _initialization;

Future<void> initializeNativeBridge({DynamicLibrary? library}) async {
  final existing = _initialization;
  if (existing != null) return existing;
  var bridge = _shared;
  if (bridge == null || bridge._closing) {
    bridge = NativeBridge(library);
    _shared = bridge;
  }
  if (bridge.initialized) return;
  final initialization = bridge.initialize();
  _initialization = initialization;
  try {
    await initialization;
  } catch (_) {
    await bridge.shutdown();
    rethrow;
  } finally {
    _initialization = null;
  }
}

NativeBridge get nativeBridge {
  if (_shared == null || _shared!._closing) _shared = NativeBridge(null);
  return _shared!;
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

  Future<Object?> nextValue() {
    if (_disposed) return Future.error(StateError('native handle is disposed'));
    return bridge.invoke(handle, 'next', const {}, subscribe: true);
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

int nativeHandle(Object? value, String kind) {
  if (value is! Map<String, dynamic> || value['kind'] != kind) {
    throw const FormatException('native result has an unexpected handle kind');
  }
  return BigInt.parse(value['handle'] as String).toSigned(64).toInt();
}

class NativeBridge {
  NativeBridge(DynamicLibrary? library) {
    bindings = SailryBindings(library ?? _library());
    _callback = NativeCallable<_Completion>.listener(_complete);
    identifier = bindings.sailry_open(_callback.nativeFunction);
    if (identifier == 0) {
      _callback.close();
      throw StateError('native bridge could not be opened');
    }
  }

  late final SailryBindings bindings;
  late final NativeCallable<_Completion> _callback;
  late final int identifier;
  final Map<int, Completer<Object?>> _pending = {};
  final Set<int> _handles = {};
  int _request = 0;
  int _received = 0;
  int? _finalCount;
  bool _closing = false;
  bool _closed = false;
  bool _disposeRequested = false;
  bool initialized = false;
  Completer<void>? _shutdown;

  Future<void> initialize() async {
    await invoke(0, 'initialize', {'iroh': _irohLibrary()});
    initialized = true;
  }

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
    final operation = method.toNativeUtf8();
    final data = utf8.encode(jsonEncode(arguments));
    final input = calloc<Uint8>(data.length);
    input.asTypedList(data.length).setAll(0, data);
    final immediate = calloc<Pointer<Uint8>>();
    final immediateLength = calloc<Uint64>();
    try {
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
            _decode(immediate.value, immediateLength.value),
          );
        } finally {
          bindings.sailry_free(immediate.value);
        }
      } else if (accepted == 0) {
        _pending.remove(request);
        completer.completeError(StateError('native operation was not accepted'));
      }
    } catch (error, stack) {
      _pending.remove(request);
      if (!completer.isCompleted) completer.completeError(error, stack);
    } finally {
      calloc.free(operation);
      calloc.free(input);
      calloc.free(immediate);
      calloc.free(immediateLength);
    }
    return completer.future;
  }

  Object? invokeSync(String method, Map<String, Object?> arguments) {
    if (_closing) throw StateError('native bridge is closed');
    final operation = method.toNativeUtf8();
    final data = utf8.encode(jsonEncode(arguments));
    final input = calloc<Uint8>(data.length);
    input.asTypedList(data.length).setAll(0, data);
    final immediate = calloc<Pointer<Uint8>>();
    final immediateLength = calloc<Uint64>();
    try {
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
      final result = _decode(immediate.value, immediateLength.value);
      if (!result.ok) {
        throw result.error ??
            StateError('native operation failed without an error');
      }
      return result.value;
    } finally {
      if (immediate.value != nullptr) bindings.sailry_free(immediate.value);
      calloc.free(operation);
      calloc.free(input);
      calloc.free(immediate);
      calloc.free(immediateLength);
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

  void _complete(int request, Pointer<Uint8> data, int length) {
    try {
      final result = _decode(data, length);
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
      bindings.sailry_free(data);
      _received++;
      _finishShutdown();
      if (_disposeRequested) _closeIfUnused();
    }
  }

  CompletionEnvelope _decode(Pointer<Uint8> data, int length) =>
      CompletionEnvelope.fromJson(
        jsonDecode(utf8.decode(data.asTypedList(length)))
            as Map<String, dynamic>,
      );

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
    _callback.close();
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

String _irohLibrary() {
  final explicit = Platform.environment['SAILRY_IROH_LIB'];
  if (explicit != null && explicit.isNotEmpty) return explicit;
  if (Platform.isIOS) {
    return '${File(Platform.resolvedExecutable).parent.path}'
        '/Frameworks/SailryIroh.framework/SailryIroh';
  }
  if (Platform.isAndroid || Platform.isLinux) return 'libsailry_iroh.so';
  if (Platform.isMacOS) return 'libsailry_iroh.dylib';
  throw UnsupportedError('Sailry Link is unavailable on this platform');
}
