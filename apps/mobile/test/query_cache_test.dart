import 'package:drift/drift.dart' show DatabaseConnection, driftRuntimeOptions;
import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/runtime/local_store.dart';
import 'package:sailry_mobile/runtime/query_cache.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  late NativeDatabase database;
  late LocalStore store;

  // Each cache opens its own store on one shared in-memory database, which
  // stands in for the file that outlives an app restart.
  LocalStore reopen() =>
      LocalStore(DatabaseConnection(database, closeStreamsSynchronously: true));

  // Reopening the same database simulates a restart, not a second owner.
  setUpAll(() => driftRuntimeOptions.dontWarnAboutMultipleDatabases = true);

  setUp(() {
    database = NativeDatabase.memory();
    store = reopen();
  });
  tearDown(() => database.close());

  test(
    'stored entries return after a restart and scopes forget them',
    () async {
      final routed = hostCacheScope('entry/node');
      final entry = hostCacheScope('entry');
      final first = QueryCache(store: store);
      first.put('${routed}view', {'a': 1}, raw: '{"a":1}');
      first.put('${entry}view', {'b': 2}, raw: '{"b":2}');
      first.put('${entry}memory', 'only', size: 4);
      await first.close();

      final second = QueryCache(store: reopen());
      expect(hostIdFromScope(routed), 'entry/node');
      expect(await second.persistedKeys(hostCachePrefix), hasLength(2));
      expect(await second.read('${routed}view', jsonObject), {'a': 1});
      expect(second.peek<Map<String, dynamic>>('${routed}view'), {'a': 1});
      expect(await second.read('${entry}memory', rawText), isNull);

      // A routed host's scope is separate from its entry Node's.
      await second.removeScope(entry);
      expect(await second.read('${entry}view', jsonObject), isNull);
      expect(await second.read('${routed}view', jsonObject), {'a': 1});
    },
  );

  test('fetch reuses a current value and shares one load', () async {
    final cache = QueryCache();
    var loads = 0;
    Future<String> load() async => 'v${++loads}';
    final results = await Future.wait([
      cache.fetch('k', load, stamp: '1'),
      cache.fetch('k', load, stamp: '1'),
    ]);
    expect(results, ['v1', 'v1']);
    expect(await cache.fetch('k', load, stamp: '1'), 'v1');
    expect(await cache.fetch('k', load, stamp: '2'), 'v2');
  });

  test('a corrupt entry is dropped instead of shown', () async {
    final cache = QueryCache(store: store);
    cache.put('host:x/view', const {}, raw: 'not json');
    await cache.flush();
    final reopened = QueryCache(store: reopen());
    expect(await reopened.read('host:x/view', jsonObject), isNull);
    expect(await reopened.persistedKeys('host:'), isEmpty);
  });

  test('the oldest entries leave first beyond the budget', () async {
    await store.write({'old': 'x' * 6});
    await Future<void>.delayed(const Duration(milliseconds: 2));
    await store.write({'new': 'y' * 6});
    await store.trim(8);
    expect(await store.keys(''), ['new']);
  });

  test('offline search finds message text and notifies subscribers', () async {
    final cache = QueryCache(store: store);
    final scope = hostCacheScope('node');
    final changes = <void>[];
    final subscription = cache.searchChanges.listen(changes.add);
    await cache.index(scope, 's1', 'transcript', [
      '修复移动端进入会话卡顿的问题',
      'Unrelated reply',
    ]);
    await cache.index(scope, 's2', 'preview', ['hello 卡顿']);
    await pumpEventQueue();
    expect(changes, isNotEmpty);

    Future<Set<String>> sessions(String query) async => {
      for (final hit in await cache.search(query)) hit.session,
    };
    expect(await sessions('进入会话'), {'s1'});
    expect(await sessions('卡顿'), {'s1', 's2'});
    expect(await sessions('unrelated'), {'s1'});
    expect(await sessions('"quoted'), isEmpty);
    expect(await sessions('50%'), isEmpty);

    // Replacing a source drops its old text; removing the scope drops all.
    await cache.index(scope, 's1', 'transcript', ['other']);
    expect(await sessions('进入会话'), isEmpty);
    await cache.removeScope(scope);
    expect(await sessions('卡顿'), isEmpty);
    await subscription.cancel();
  });
}
