import 'dart:async';

import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

/// The data layer between pages and host connections.
///
/// Pages read a key here first. A cached value renders immediately while the
/// owner refreshes it in the background; a missing value is fetched once,
/// stored and then returned. Live observations write their latest view under
/// the same keys, so a reopened page starts from what was last seen.
///
/// Values are host projections, not authoritative state: a cached view never
/// enables a write until its live observation confirms the connection.
/// Persisted entries survive restarts within [diskBudget]; removing a host or
/// losing access to it drops its whole scope from memory and disk.
class QueryCache {
  QueryCache({
    this.directory,
    this.memoryBudget = 32 * 1024 * 1024,
    this.diskBudget = 64 * 1024 * 1024,
    this.writeDelay = const Duration(seconds: 2),
  });

  /// Where persisted entries live; null keeps every entry in memory only.
  final Directory? directory;
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

  /// The value in memory, without touching disk or the network.
  T? peek<T>(String key) {
    final entry = _entries.remove(key);
    if (entry == null) return null;
    _entries[key] = entry;
    final value = entry.value;
    if (value is! T) return null;
    return value as T;
  }

  /// The value's stamp, used by readers to decide whether it is current.
  String? stamp(String key) => _entries[key]?.stamp;

  /// The value in memory, else the persisted copy decoded off the UI thread.
  Future<T?> read<T extends Object>(
    String key,
    T Function(String raw) decode,
  ) async {
    final cached = peek<T>(key);
    if (cached != null) return cached;
    final file = _file(key);
    if (file == null) return null;
    final loading = _loading[key];
    if (loading != null) return await loading as T?;
    final load = () async {
      try {
        final raw = await file.readAsString();
        final value = await _decode(decode, raw);
        // A newer live value or a removal wins over a slower disk read.
        if (_closed || _removed.contains(key) || _entries.containsKey(key)) {
          return peek<T>(key);
        }
        _store(key, _Entry(value, raw.length, null));
        return value;
      } on FileSystemException {
        return null;
      } on FormatException {
        await _delete(key);
        return null;
      }
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
    if (raw != null && directory != null) {
      _pending[key] = raw;
      _writeTimer ??= Timer(writeDelay, () => unawaited(flush()));
    }
  }

  /// Returns the cached value when [current] accepts it, else loads it once
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

  /// Every key below [prefix], from memory and disk.
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
    final root = directory;
    if (root == null) return;
    await _writing;
    await for (final (file, key) in _files(root)) {
      if (!key.startsWith(prefix)) continue;
      try {
        await file.delete();
        final parent = file.parent;
        if (await parent.list().isEmpty) await parent.delete();
      } on FileSystemException {
        // Already absent.
      }
    }
  }

  /// Keys below [prefix] that have a persisted copy.
  Future<List<String>> persistedKeys(String prefix) async {
    final root = directory;
    if (root == null) return const [];
    return [
      await for (final (_, key) in _files(root))
        if (key.startsWith(prefix)) key,
    ];
  }

  /// Writes pending entries now, for example before the app is suspended.
  Future<void> flush() {
    _writeTimer?.cancel();
    _writeTimer = null;
    final root = directory;
    if (root == null || _pending.isEmpty) return _writing;
    final batch = Map.of(_pending);
    _pending.clear();
    return _writing = _writing.then((_) async {
      try {
        for (final MapEntry(:key, :value) in batch.entries) {
          if (_removed.contains(key)) continue;
          final file = _file(key)!;
          await file.parent.create(recursive: true);
          final temporary = File('${file.path}.tmp');
          await temporary.writeAsString(value, flush: true);
          await temporary.rename(file.path);
        }
        await _trimDisk(root);
      } on FileSystemException catch (error) {
        // A cache that cannot be written only costs the next cold start.
        stderr.writeln('Query cache write failed: $error');
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

  Future<void> _trimDisk(Directory root) async {
    final files = <(File, FileStat)>[];
    var total = 0;
    await for (final (file, _) in _files(root)) {
      final stat = await file.stat();
      files.add((file, stat));
      total += stat.size;
    }
    if (total <= diskBudget) return;
    files.sort((a, b) => a.$2.modified.compareTo(b.$2.modified));
    for (final (file, stat) in files) {
      if (total <= diskBudget) break;
      await file.delete();
      total -= stat.size;
    }
  }

  Future<void> _delete(String key) async {
    try {
      await _file(key)?.delete();
    } on FileSystemException {
      // Already absent.
    }
  }

  // A key's scope, up to its first `/`, names a directory and the rest its
  // file, which keeps each encoded name well under file name limits.
  File? _file(String key) {
    final root = directory;
    if (root == null) return null;
    final split = key.indexOf('/') + 1;
    final separator = Platform.pathSeparator;
    return File(
      '${root.path}$separator${_name(key.substring(0, split))}'
      '$separator${_name(key.substring(split))}',
    );
  }

  /// Persisted entries with their keys; temporary and foreign files are skipped.
  static Stream<(File, String)> _files(Directory root) async* {
    try {
      await for (final scope in root.list()) {
        if (scope is! Directory) continue;
        final prefix = _decodeName(
          scope.uri.pathSegments.lastWhere((segment) => segment.isNotEmpty),
        );
        if (prefix == null) continue;
        await for (final file in scope.list()) {
          if (file is! File) continue;
          final rest = _decodeName(file.uri.pathSegments.last);
          if (rest != null) yield (file, '$prefix$rest');
        }
      }
    } on FileSystemException {
      return;
    }
  }

  static String _name(String part) => part.isEmpty
      ? '_'
      : base64Url.encode(utf8.encode(part)).replaceAll('=', '');

  static String? _decodeName(String name) {
    if (name == '_') return '';
    if (name.contains('.')) return null;
    try {
      return utf8.decode(base64Url.decode(base64Url.normalize(name)));
    } on FormatException {
      return null;
    }
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
