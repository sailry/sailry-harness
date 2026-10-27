import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_output.dart';

void main() {
  Map<String, dynamic> call([String response = 'result']) => {
    'source': {'entry': 'request', 'index': 0},
    'response': {'entry': response, 'index': 0},
    'details': true,
  };

  test('loads only visible connected details once', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    var reads = 0;
    Future<Map<String, dynamic>?> load(String entry, int index) async {
      expect((entry, index), ('request', 0));
      reads++;
      return {'preview': 'Original output'};
    }

    for (final (expanded, enabled) in [(false, true), (true, false)]) {
      await output.resolve(
        call(),
        expanded: expanded,
        enabled: enabled,
        load: load,
      );
    }
    await output.resolve(
      {...call(), 'details': false},
      expanded: true,
      enabled: true,
      load: load,
    );
    expect(reads, 0);
    await output.resolve(call(), expanded: true, enabled: true, load: load);
    await output.resolve(call(), expanded: true, enabled: true, load: load);
    expect(reads, 1);
    expect(output.value, {'preview': 'Original output'});
  });

  test('unavailable output requires explicit retry', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    var reads = 0;
    Future<Map<String, dynamic>?> load(String _, int _) async =>
        ++reads == 1 ? null : {'preview': 'Recovered output'};

    await output.resolve(call(), expanded: true, enabled: true, load: load);
    expect(output.value, isNull);
    await output.resolve(call(), expanded: true, enabled: false, load: load);
    expect(reads, 1);
    await output.resolve(call(), expanded: true, enabled: true, load: load);
    expect(reads, 1);
    expect(output.failed, isTrue);
    await output.resolve(
      call(),
      expanded: true,
      enabled: true,
      load: load,
      retry: true,
    );
    expect(reads, 2);
    expect(output.value, {'preview': 'Recovered output'});
  });

  test('loads current approval arguments before a result', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    final arguments = {'path': 'notes.txt', 'text': 'Requested content'};
    final approval = {
      ...call(),
      'response': null,
      'details': false,
      'approval': {'state': 'pending'},
    };
    await output.resolve(
      approval,
      expanded: false,
      enabled: true,
      load: (entry, index) async {
        expect((entry, index), ('request', 0));
        return {'arguments': arguments};
      },
    );
    expect(output.value?['arguments'], arguments);
  });

  test('approval updates do not repeat a failed read', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    var reads = 0;
    final approval = {
      ...call(),
      'response': null,
      'approval': {'state': 'pending'},
    };
    Future<Map<String, dynamic>?> load(String _, int _) async {
      reads++;
      throw StateError('Details unavailable');
    }

    await expectLater(
      output.resolve(approval, expanded: false, enabled: true, load: load),
      throwsStateError,
    );
    for (var index = 0; index < 5; index++) {
      await output.resolve(
        approval,
        expanded: false,
        enabled: true,
        load: load,
      );
    }
    expect(reads, 1);
    expect(output.failed, isTrue);
    await output.resolve(
      approval,
      expanded: false,
      enabled: true,
      retry: true,
      load: (_, _) async => {
        'arguments': {'path': 'captured.txt'},
      },
    );
    expect(output.value?['arguments'], {'path': 'captured.txt'});
    expect(output.failed, isFalse);
  });

  test('a changed revision clears the failed identity', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    await expectLater(
      output.resolve(
        {...call(), 'revision': 'old'},
        expanded: true,
        enabled: true,
        load: (_, _) async => throw StateError('Details unavailable'),
      ),
      throwsStateError,
    );
    await output.resolve(
      {...call(), 'revision': 'new'},
      expanded: true,
      enabled: true,
      load: (_, _) async => {
        'arguments': {'path': 'current.txt'},
      },
    );
    expect(output.value?['arguments'], {'path': 'current.txt'});
    expect(output.failed, isFalse);
  });

  test('coalesces reads and rejects replaced output', () async {
    final output = DeferredOutput();
    addTearDown(output.dispose);
    final first = Completer<Map<String, dynamic>?>();
    var reads = 0;
    Future<Map<String, dynamic>?> load(String _, int _) {
      reads++;
      return first.future;
    }

    final pending = output.resolve(
      call(),
      expanded: true,
      enabled: true,
      load: load,
    );
    await output.resolve(call(), expanded: true, enabled: true, load: load);
    expect(reads, 1);
    await output.resolve(
      call('replacement'),
      expanded: true,
      enabled: true,
      load: (_, _) async => {'preview': 'Replacement'},
    );
    first.complete({'preview': 'Stale result'});
    await pending;
    expect(output.value, {'preview': 'Replacement'});
  });

  test(
    'ignores stale failures and explicitly retries current errors',
    () async {
      final output = DeferredOutput();
      addTearDown(output.dispose);
      final first = Completer<Map<String, dynamic>?>();
      final pending = output.resolve(
        call(),
        expanded: true,
        enabled: true,
        load: (_, _) => first.future,
      );
      output.bind(call('replacement'));
      first.completeError(StateError('Stale result'));
      await pending;
      expect(output.value, isNull);
      await expectLater(
        output.resolve(
          call('replacement'),
          expanded: true,
          enabled: true,
          load: (_, _) async => throw StateError('Current error'),
        ),
        throwsStateError,
      );
      await output.resolve(
        call('replacement'),
        expanded: true,
        enabled: true,
        retry: true,
        load: (_, _) async => {'preview': 'Retried output'},
      );
      expect(output.value, {'preview': 'Retried output'});
    },
  );

  test('closing the owner discards pending output', () async {
    final output = DeferredOutput();
    var changes = 0;
    output.addListener(() => changes++);
    final first = Completer<Map<String, dynamic>?>();
    final pending = output.resolve(
      call(),
      expanded: true,
      enabled: true,
      load: (_, _) => first.future,
    );
    output.dispose();
    first.complete({'preview': 'Closed output'});
    await pending;
    expect(changes, 0);
    expect(output.value, isNull);
  });
}
