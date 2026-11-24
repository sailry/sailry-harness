import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/runtime/query_cache.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  late Directory root;
  setUp(() async => root = await Directory.systemTemp.createTemp('query'));
  tearDown(() => root.delete(recursive: true));

  test(
    'persisted entries return after a restart and scopes forget them',
    () async {
      final routed = hostCacheScope('entry/node');
      final entry = hostCacheScope('entry');
      final first = QueryCache(directory: root);
      first.put('${routed}view', {'a': 1}, raw: '{"a":1}');
      first.put('${entry}view', {'b': 2}, raw: '{"b":2}');
      first.put('${entry}memory', 'only', size: 4);
      await first.close();

      final second = QueryCache(directory: root);
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
    final cache = QueryCache(directory: root);
    cache.put('host:x/view', const {}, raw: 'not json');
    await cache.flush();
    final reopened = QueryCache(directory: root);
    expect(await reopened.read('host:x/view', jsonObject), isNull);
    expect(await reopened.persistedKeys('host:'), isEmpty);
  });
}
