import 'dart:async';
import 'dart:convert';
import 'dart:isolate';

import 'package:flutter/foundation.dart';

import 'local_store.dart';

/// The data layer between pages and host connections.
///
/// Pages read a key here first. A cached value renders immediately while the
/// owner refreshes it in the background. Concurrent reads share one disk load.
/// Live observations write their latest view under
/// the same keys, so a reopened page starts from what was last seen.
///
/// Values are host projections, not authoritative state: a cached view never
/// enables a write until its live observation confirms the connection.
/// Recently used values stay decoded in memory within [memoryBudget]; entries
/// written with their encoded form persist on disk within [diskBudget].
/// Removing a host or losing access to it drops its whole scope.
class QueryCache {
  QueryCache({
    LocalStore? store,
    this.memoryBudget = 32 * 1024 * 1024,
    this.diskBudget = 64 * 1024 * 1024,
    this.writeDelay = const Duration(seconds: 2),
  }) : _database = store;

  final LocalStore? _database;
  final int memoryBudget;
  final int diskBudget;
  final Duration writeDelay;

  final _entries = <String, _Entry>{};
  final _loading = <String, Future<Object?>>{};
  final _pending = <String, String>{};
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
    final database = _database;
    if (_closed || database == null) return null;
    final loading = _loading[key];
    if (loading != null) return await loading as T?;
    late final Future<T?> load;
    load = _writing.then((_) async {
      if (!identical(_loading[key], load)) return null;
      final raw =
          _pending[key] ?? await _onDisk<String?>(() => database.value(key));
      if (raw == null || !identical(_loading[key], load)) return null;
      final T value;
      try {
        value = await _decode(decode, raw);
      } on FormatException {
        if (identical(_loading[key], load)) {
          _pending.remove(key);
          await _write(() => database.removeEntry(key));
        }
        return null;
      }
      // A newer live value or a removal retires a slower read.
      if (!identical(_loading[key], load)) return null;
      _store(key, _Entry(value, raw.length));
      return value;
    });
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
  void put(String key, Object value, {String? raw, int? size}) {
    if (_closed) return;
    _loading.remove(key);
    _store(key, _Entry(value, raw?.length ?? size ?? 1024));
    if (raw != null && _database != null) {
      _pending[key] = raw;
      _writeTimer ??= Timer(writeDelay, () => unawaited(flush()));
    }
  }

  /// Every key below [prefix], from memory and the database.
  Future<void> removeScope(String prefix) {
    _loading.removeWhere((key, _) => key.startsWith(prefix));
    _pending.removeWhere((key, _) => key.startsWith(prefix));
    _entries.removeWhere((key, value) {
      if (!key.startsWith(prefix)) return false;
      _bytes -= value.size;
      return true;
    });
    final database = _database;
    if (database == null || _closed) return Future.value();
    return _write(() => database.removeScope(prefix));
  }

  /// Keys below [prefix] that have a stored copy.
  Future<List<String>> persistedKeys(String prefix) async {
    final database = _database;
    if (database == null || _closed) return const [];
    await _writing;
    return await _onDisk(() => database.keys(prefix)) ?? const [];
  }

  /// Replaces the message text [source] contributed to the offline search
  /// index for one session; an empty [bodies] removes it.
  Future<void> index(
    String scope,
    String session,
    String source,
    List<String> bodies,
  ) async {
    final database = _database;
    if (database == null || _closed) return;
    await _write(
      () => database.index(scope, session, source, bodies, budget: diskBudget),
    );
  }

  /// Indexed sessions whose messages contain [query].
  Future<List<({String session, String body})>> search(
    String query, {
    required String scope,
    required Set<String> sessions,
  }) async {
    final database = _database;
    if (database == null || _closed || sessions.isEmpty) return const [];
    await _writing;
    return await _onDisk(
          () => database.search(query, scope: scope, sessions: sessions),
        ) ??
        const [];
  }

  /// Emits after the search index changes.
  Stream<void> get searchChanges =>
      _database?.searchChanges ?? const Stream.empty();

  /// Writes pending entries now, for example before the app is suspended.
  Future<void> flush() {
    _writeTimer?.cancel();
    _writeTimer = null;
    final database = _database;
    if (database == null || _pending.isEmpty) return _writing;
    final batch = Map.of(_pending);
    _pending.clear();
    return _write(() async {
      await database.write(batch);
      await database.trim(diskBudget);
    });
  }

  Future<void> _write(Future<void> Function() action) =>
      _writing = _writing.then((_) async {
        await _onDisk(action);
      });

  Future<T?> _onDisk<T>(Future<T> Function() action) async {
    try {
      return await action();
    } catch (error) {
      debugPrint('Local cache unavailable: ${error.runtimeType}');
      return null;
    }
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _loading.clear();
    _entries.clear();
    _bytes = 0;
    await flush();
    final database = _database;
    if (database != null) await _onDisk(database.close);
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
}

class _Entry {
  _Entry(this.value, this.size);
  final Object value;
  final int size;
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
