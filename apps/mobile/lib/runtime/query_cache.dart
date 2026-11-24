import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:flutter/foundation.dart';

import 'local_store.dart';

/// The data layer between pages and host connections.
///
/// Pages read a key here first. A cached value renders immediately while the
/// owner refreshes it in the background; a missing value is fetched once,
/// stored and then returned. Live observations write their latest view under
/// the same keys, so a reopened page starts from what was last seen.
///
/// Values are host projections, not authoritative state: a cached view never
/// enables a write until its live observation confirms the connection.
/// Recently used values stay decoded in memory within [memoryBudget]; entries
/// written with their encoded form persist in [store] within [diskBudget].
/// Removing a host or losing access to it drops its whole scope.
class QueryCache {
  QueryCache({
    this.store,
    this.memoryBudget = 32 * 1024 * 1024,
    this.diskBudget = 64 * 1024 * 1024,
    this.writeDelay = const Duration(seconds: 2),
  });

  /// The local database; null keeps every entry in memory only.
  final LocalStore? store;
  final int memoryBudget;
  final int diskBudget;
  final Duration writeDelay;

  final _entries = <String, _Entry>{};
  final _loading = <String, Future<Object?>>{};
  final _pending = <String, String>{};
  final _removed = <String>{};
  int _bytes = 0;
  Timer? _writeTimer;
  Future<void> _writing = Future.value();
  bool _closed = false;

  /// The value in memory, without touching the database or the network.
  T? peek<T>(String key) {
    final entry = _entries.remove(key);
    if (entry == null) return null;
    _entries[key] = entry;
    final value = entry.value;
    if (value is! T) return null;
    return value as T;
  }

  /// The value in memory, else the stored copy decoded off the UI thread.
  Future<T?> read<T extends Object>(
    String key,
    T Function(String raw) decode,
  ) async {
    final cached = peek<T>(key);
    if (cached != null) return cached;
    final database = store;
    if (database == null) return null;
    final loading = _loading[key];
    if (loading != null) return await loading as T?;
    final load = () async {
      final raw = _pending[key] ?? await database.value(key);
      if (raw == null) return null;
      final T value;
      try {
        value = await _decode(decode, raw);
      } on FormatException {
        await _forget(key);
        return null;
      }
      // A newer live value or a removal wins over a slower read.
      if (_closed || _removed.contains(key) || _entries.containsKey(key)) {
        return peek<T>(key);
      }
      _store(key, _Entry(value, raw.length, null));
      return value;
    }();
    _loading[key] = load;
    try {
      return await load;
    } finally {
      if (identical(_loading[key], load)) _loading.remove(key);
    }
  }

  // Kept static so the isolate closure captures only the decoder and text.
  static Future<T> _decode<T>(T Function(String) decode, String raw) async =>
      raw.length < 32 * 1024 ? decode(raw) : Isolate.run(() => decode(raw));

  /// Stores [value]. With [raw], the encoded form is also persisted.
  void put(String key, Object value, {String? raw, int? size, String? stamp}) {
    if (_closed) return;
    _removed.remove(key);
    _store(key, _Entry(value, raw?.length ?? size ?? 1024, stamp));
    if (raw != null && store != null) {
      _pending[key] = raw;
      _writeTimer ??= Timer(writeDelay, () => unawaited(flush()));
    }
  }

  /// Returns the cached value when its [stamp] matches, else loads it once
  /// for every concurrent caller and caches the result under [stamp].
  Future<T> fetch<T extends Object>(
    String key,
    Future<T> Function() load, {
    String? stamp,
  }) {
    final cached = peek<T>(key);
    if (cached != null && _entries[key]?.stamp == stamp) {
      return Future.value(cached);
    }
    final running = _loading[key];
    if (running != null) return running.then((value) => value as T);
    final future = load().then((value) {
      if (!_closed && !_removed.contains(key)) {
        put(key, value, stamp: stamp);
      }
      return value;
    });
    _loading[key] = future;
    void done() {
      if (identical(_loading[key], future)) _loading.remove(key);
    }

    future.then((_) => done(), onError: (Object _) => done());
    return future;
  }

  /// Every key below [prefix], from memory and the database.
  Future<void> removeScope(String prefix) async {
    final keys = [
      ..._entries.keys.where((key) => key.startsWith(prefix)),
      ..._pending.keys.where((key) => key.startsWith(prefix)),
    ];
    for (final key in keys) {
      _removed.add(key);
      _pending.remove(key);
      final entry = _entries.remove(key);
      if (entry != null) _bytes -= entry.size;
    }
    final database = store;
    if (database == null) return;
    await _writing;
    await database.removeScope(prefix);
  }

  /// Keys below [prefix] that have a stored copy.
  Future<List<String>> persistedKeys(String prefix) async =>
      await store?.keys(prefix) ?? const [];

  /// Replaces the message text [source] contributed to the offline search
  /// index for one session; an empty [bodies] removes it.
  Future<void> index(
    String scope,
    String session,
    String source,
    List<String> bodies,
  ) async {
    if (_closed) return;
    await store?.index(scope, session, source, bodies);
  }

  /// Indexed sessions whose messages contain [query].
  Future<List<({String scope, String session, String body})>> search(
    String query,
  ) async => await store?.search(query) ?? const [];

  /// Emits after the search index changes.
  Stream<void> get searchChanges =>
      store?.changes(search: true) ?? const Stream.empty();

  /// Writes pending entries now, for example before the app is suspended.
  Future<void> flush() {
    _writeTimer?.cancel();
    _writeTimer = null;
    final database = store;
    if (database == null || _pending.isEmpty) return _writing;
    final batch = Map.of(_pending);
    _pending.clear();
    return _writing = _writing.then((_) async {
      batch.removeWhere((key, _) => _removed.contains(key));
      try {
        await database.write(batch);
        await database.trim(diskBudget);
      } catch (error) {
        // A cache that cannot be written only costs the next cold start.
        debugPrint('Local store write failed: $error');
      }
    });
  }

  Future<void> close() async {
    await flush();
    _closed = true;
  }

  void _store(String key, _Entry entry) {
    final previous = _entries.remove(key);
    if (previous != null) _bytes -= previous.size;
    _entries[key] = entry;
    _bytes += entry.size;
    while (_bytes > memoryBudget && _entries.length > 1) {
      final oldest = _entries.keys.first;
      _bytes -= _entries.remove(oldest)!.size;
    }
  }

  Future<void> _forget(String key) async {
    _pending.remove(key);
    await store?.removeScope(key);
  }
}

class _Entry {
  _Entry(this.value, this.size, this.stamp);
  final Object value;
  final int size;
  final String? stamp;
}

/// Decoders for [QueryCache.read]. Large entries decode on another isolate,
/// so a decoder must be a top-level or static function.
String rawText(String raw) => raw;
Map<String, dynamic> jsonObject(String raw) {
  final value = jsonDecode(raw);
  if (value is! Map<String, dynamic>) {
    throw const FormatException('cached entry is not an object');
  }
  return value;
}
