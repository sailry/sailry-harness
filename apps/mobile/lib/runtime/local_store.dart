import 'dart:async';
import 'dart:io';

import 'package:drift/drift.dart';
import 'package:drift_flutter/drift_flutter.dart';

/// Mobile's local database: the last views received from hosts and a
/// full-text index of the messages seen in them.
///
/// It holds projections the Client already delivered, never authoritative
/// state, so any of it can be dropped and observed again. Each host's rows
/// share a scope prefix that is removed when the host is forgotten or access
/// to it ends.
///
/// The schema is two tables, so it is declared in SQL rather than through
/// drift's code generator; readers subscribe to [changes] instead of
/// generated stream queries.
class LocalStore extends GeneratedDatabase {
  LocalStore(super.executor);

  /// Opens `local.sqlite` in [directory] on drift's background isolate.
  factory LocalStore.open(String directory) => LocalStore(
    driftDatabase(
      name: 'local',
      native: DriftNativeOptions(
        databaseDirectory: () async => Directory(directory),
      ),
    ),
  );

  static const _entries = 'entries';
  static const _search = 'message_search';

  @override
  Iterable<TableInfo<Table, Object?>> get allTables => const [];

  @override
  int get schemaVersion => 1;

  @override
  MigrationStrategy get migration => MigrationStrategy(
    onCreate: (_) async {
      await customStatement('''
        CREATE TABLE $_entries (
          key TEXT NOT NULL PRIMARY KEY,
          value TEXT NOT NULL,
          size INTEGER NOT NULL,
          touched_ms INTEGER NOT NULL
        ) WITHOUT ROWID''');
      await customStatement(
        'CREATE INDEX entries_touched ON $_entries (touched_ms)',
      );
      // Trigram tokens match any substring of three or more characters,
      // which also covers Chinese and other text without word separators.
      await customStatement('''
        CREATE VIRTUAL TABLE $_search USING fts5(
          scope UNINDEXED,
          session UNINDEXED,
          source UNINDEXED,
          body,
          tokenize = 'trigram'
        )''');
    },
  );

  /// Emits after entries or the search index change.
  Stream<void> changes({bool search = false}) =>
      tableUpdates(TableUpdateQuery.onTableName(search ? _search : _entries));

  Future<String?> value(String key) async {
    final rows = await customSelect(
      'SELECT value FROM $_entries WHERE key = ?',
      variables: [Variable.withString(key)],
    ).get();
    return rows.firstOrNull?.read<String>('value');
  }

  /// Stores each value under its key in one transaction.
  Future<void> write(Map<String, String> values) async {
    if (values.isEmpty) return;
    final now = DateTime.now().millisecondsSinceEpoch;
    await transaction(() async {
      for (final MapEntry(:key, :value) in values.entries) {
        await customStatement(
          'INSERT INTO $_entries (key, value, size, touched_ms) '
          'VALUES (?, ?, ?, ?) ON CONFLICT (key) DO UPDATE SET '
          'value = excluded.value, size = excluded.size, '
          'touched_ms = excluded.touched_ms',
          [key, value, value.length, now],
        );
      }
    });
    notifyUpdates({const TableUpdate(_entries)});
  }

  Future<List<String>> keys(String prefix) async {
    final rows = await customSelect(
      'SELECT key FROM $_entries WHERE key >= ? AND key < ?',
      variables: _range(prefix),
    ).get();
    return [for (final row in rows) row.read<String>('key')];
  }

  /// Every entry and indexed message under [prefix].
  Future<void> removeScope(String prefix) async {
    await transaction(() async {
      await customStatement(
        'DELETE FROM $_entries WHERE key >= ? AND key < ?',
        [for (final value in _range(prefix)) value.value],
      );
      await customStatement(
        'DELETE FROM $_search WHERE scope >= ? AND scope < ?',
        [for (final value in _range(prefix)) value.value],
      );
    });
    notifyUpdates({const TableUpdate(_entries), const TableUpdate(_search)});
  }

  /// Removes the least recently written entries beyond [budget] characters.
  Future<void> trim(int budget) async {
    final rows = await customSelect(
      'SELECT key, size FROM $_entries ORDER BY touched_ms DESC',
    ).get();
    var total = 0;
    final stale = <String>[];
    for (final row in rows) {
      total += row.read<int>('size');
      if (total > budget) stale.add(row.read<String>('key'));
    }
    if (stale.isEmpty) return;
    await transaction(() async {
      for (final key in stale) {
        await customStatement('DELETE FROM $_entries WHERE key = ?', [key]);
      }
    });
    notifyUpdates({const TableUpdate(_entries)});
  }

  /// Replaces the indexed text [source] contributed for one session.
  Future<void> index(
    String scope,
    String session,
    String source,
    List<String> bodies,
  ) async {
    await transaction(() async {
      await customStatement(
        'DELETE FROM $_search WHERE scope = ? AND session = ? AND source = ?',
        [scope, session, source],
      );
      for (final body in bodies) {
        if (body.trim().isEmpty) continue;
        await customStatement(
          'INSERT INTO $_search (scope, session, source, body) '
          'VALUES (?, ?, ?, ?)',
          [scope, session, source, body],
        );
      }
    });
    notifyUpdates({const TableUpdate(_search)});
  }

  /// Sessions whose indexed messages contain [query], newest index first,
  /// with the text that matched.
  Future<List<({String scope, String session, String body})>> search(
    String query, {
    int limit = 200,
  }) async {
    final needle = query.trim();
    if (needle.isEmpty) return const [];
    // Trigrams need three characters; shorter text scans with LIKE.
    final rows = needle.runes.length >= 3
        ? await customSelect(
            'SELECT scope, session, body FROM $_search '
            'WHERE $_search MATCH ? ORDER BY rowid DESC LIMIT ?',
            variables: [
              Variable.withString('body : "${needle.replaceAll('"', '""')}"'),
              Variable.withInt(limit),
            ],
          ).get()
        : await customSelect(
            'SELECT scope, session, body FROM $_search '
            "WHERE body LIKE ? ESCAPE '\\' ORDER BY rowid DESC LIMIT ?",
            variables: [
              Variable.withString('%${_escapeLike(needle)}%'),
              Variable.withInt(limit),
            ],
          ).get();
    return [
      for (final row in rows)
        (
          scope: row.read<String>('scope'),
          session: row.read<String>('session'),
          body: row.read<String>('body'),
        ),
    ];
  }

  static List<Variable<String>> _range(String prefix) => [
    Variable.withString(prefix),
    Variable.withString('$prefix\u{10FFFF}'),
  ];

  static String _escapeLike(String value) => value
      .replaceAll('\\', '\\\\')
      .replaceAll('%', '\\%')
      .replaceAll('_', '\\_');
}
