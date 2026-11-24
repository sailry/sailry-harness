import 'dart:async';
import 'dart:io';

import 'package:drift/native.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/runtime/local_store.dart';
import 'package:sailry_mobile/runtime/query_cache.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory directory;
  LocalStore reopen() =>
      LocalStore(NativeDatabase(File('${directory.path}/local.sqlite')));
  QueryCache useCache([LocalStore? store]) {
    final cache = QueryCache(store: store ?? reopen());
    addTearDown(cache.close);
    return cache;
  }

  setUp(() async => directory = await Directory.systemTemp.createTemp('cache'));
  tearDown(() => directory.delete(recursive: true));

  test(
    'stored entries return after a restart and scopes forget them',
    () async {
      final routed = hostCacheScope('entry/node');
      final entry = hostCacheScope('entry');
      final first = useCache();
      first.put('${routed}view', {'a': 1}, raw: '{"a":1}');
      first.put('${entry}view', {'b': 2}, raw: '{"b":2}');
      first.put('${entry}memory', 'only', size: 4);
      await first.close();

      final second = useCache();
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

  test(
    'concurrent cold reads share one load then reuse the decoded value',
    () async {
      final store = reopen();
      final cache = useCache(store);
      await store.write({'key': 'value'});
      var decodes = 0;
      String decode(String raw) {
        decodes++;
        return raw;
      }

      final reads = await Future.wait([
        cache.read('key', decode),
        cache.read('key', decode),
      ]);
      expect(reads, ['value', 'value']);
      expect(await cache.read('key', decode), 'value');
      expect(decodes, 1);
    },
  );

  test('a corrupt entry is dropped instead of shown', () async {
    final cache = useCache();
    cache.put('host:x/view', const {}, raw: 'not json');
    cache.put('host:x/view-other', 'another entry', raw: 'another entry');
    await cache.close();
    final reopened = useCache();
    expect(await reopened.read('host:x/view', jsonObject), isNull);
    expect(await reopened.persistedKeys('host:'), ['host:x/view-other']);
  });

  test('the oldest entries leave first beyond the budget', () async {
    final store = reopen();
    addTearDown(store.close);
    await store.write({'old': 'x' * 6});
    await Future<void>.delayed(const Duration(milliseconds: 2));
    await store.write({'new': 'y' * 6});
    await store.trim(8);
    expect(await store.keys(''), ['new']);
  });

  test('offline search finds message text and notifies subscribers', () async {
    final cache = useCache();
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
      for (final hit in await cache.search(
        query,
        scope: scope,
        sessions: const {'s1', 's2'},
      ))
        hit.session,
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

  test(
    'offline search limits eligible sessions after finding matches',
    () async {
      final cache = useCache();
      final scope = hostCacheScope('selected');
      await cache.index(scope, 'old', 'preview', [
        'needle from the older session',
      ]);
      await cache.index(
        scope,
        'new',
        'transcript',
        List.generate(201, (index) => 'needle from message $index'),
      );
      await cache.index(scope, 'excluded', 'preview', ['needle']);
      await cache.index(hostCacheScope('other'), 'old', 'preview', ['needle']);

      expect(
        await cache.search(
          'needle',
          scope: scope,
          sessions: const {'old', 'new'},
        ),
        [
          (session: 'new', body: 'needle from message 200'),
          (session: 'old', body: 'needle from the older session'),
        ],
      );
    },
  );

  test(
    'the search index drops its oldest text beyond the disk budget',
    () async {
      final cache = QueryCache(store: reopen(), diskBudget: 20);
      addTearDown(cache.close);
      final scope = hostCacheScope('node');
      await cache.index(scope, 'old', 'transcript', [
        'needle from the old turn',
      ]);
      await cache.index(scope, 'first', 'transcript', ['first needle']);
      await cache.index(scope, 'latest', 'transcript', ['latest needle']);

      expect(
        await cache.search(
          'needle',
          scope: scope,
          sessions: const {'old', 'first', 'latest'},
        ),
        [(session: 'latest', body: 'latest needle')],
      );
    },
  );

  test(
    'removing a scope retires in-flight reads before it can be reused',
    () async {
      final store = _PendingReadStore();
      final cache = useCache(store);
      final scope = hostCacheScope('node');
      final key = '${scope}view';
      await store.write({key: 'old'});
      final read = cache.read(key, rawText);
      await store.readStarted.future;
      await cache.removeScope(scope);
      cache.put(key, 'new', raw: 'new');
      store.finishRead.complete();

      expect(await read, isNull);
      expect(cache.peek<String>(key), 'new');
      await cache.flush();
      expect(await store.value(key), 'new');
    },
  );

  test('a queued write cannot restore a forgotten scope', () async {
    final store = _PendingWriteStore();
    final cache = useCache(store);
    final scope = hostCacheScope('node');
    final key = '${scope}view';
    cache.put(key, 'old', raw: 'old');
    final write = cache.flush();
    await store.writeStarted.future;
    final removal = cache.removeScope(scope);
    store.finishWrite.complete();
    await Future.wait([write, removal]);

    expect(cache.peek<String>(key), isNull);
    expect(await store.value(key), isNull);
  });

  test(
    'unavailable cache storage does not block startup or in-memory values',
    () async {
      final cache = useCache(_UnavailableStore());
      final session = AppSession.test(
        ready: false,
        cache: cache,
        savedPeers: (_) async => false,
      );
      addTearDown(session.dispose);
      await session.start(path: '${directory.path}/controller');
      expect(session.ready, isTrue);
      expect(session.error, isNull);

      expect(await cache.read('missing', rawText), isNull);
      expect(await cache.persistedKeys(hostCachePrefix), isEmpty);
      cache.put('live', 'current', raw: 'current');
      await cache.flush();
      expect(cache.peek<String>('live'), 'current');
      await session.close();
    },
  );
}

class _PendingReadStore extends LocalStore {
  _PendingReadStore() : super(NativeDatabase.memory());

  final readStarted = Completer<void>();
  final finishRead = Completer<void>();

  @override
  Future<String?> value(String key) async {
    final value = await super.value(key);
    if (!readStarted.isCompleted) {
      readStarted.complete();
      await finishRead.future;
    }
    return value;
  }
}

class _PendingWriteStore extends LocalStore {
  _PendingWriteStore() : super(NativeDatabase.memory());

  final writeStarted = Completer<void>();
  final finishWrite = Completer<void>();

  @override
  Future<void> write(Map<String, String> values) async {
    writeStarted.complete();
    await finishWrite.future;
    await super.write(values);
  }
}

class _UnavailableStore extends LocalStore {
  _UnavailableStore() : super(NativeDatabase.memory());

  @override
  Future<String?> value(String key) async => throw StateError('unavailable');

  @override
  Future<List<String>> keys(String prefix) async =>
      throw StateError('unavailable');

  @override
  Future<void> write(Map<String, String> values) async =>
      throw StateError('unavailable');

  @override
  Future<void> removeScope(String prefix) async =>
      throw StateError('unavailable');
}
