import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';

Map<String, dynamic> view(
  List<Map<String, dynamic>> items, {
  Map<String, dynamic> guidance = const {},
  bool paused = false,
  Object? reason,
}) => {
  'snapshot': {
    'page': {
      'queue': {'items': items, 'paused': paused, 'pause_reason': ?reason},
    },
  },
  'guidance': guidance,
};

void main() {
  group('summarize', () {
    test('is empty without queued input', () {
      expect(summarize({}, null), (
        count: 0,
        paused: false,
        reason: null,
        guiding: false,
      ));
    });

    test('marks pending guidance among queued input', () {
      final state = summarize(
        view(
          [
            {'turn': 'next'},
            {'turn': 'guide', 'guidance': 'running'},
          ],
          guidance: {'guide': 'pending'},
        ),
        null,
      );
      expect(state, (count: 2, paused: false, reason: null, guiding: true));
    });

    test('counts guidance until its turn is projected', () {
      const GuidancePreview admitting = (
        turn: null,
        text: 'Focus',
        attachments: [],
      );
      const GuidancePreview admitted = (
        turn: 'guide',
        text: 'Focus',
        attachments: [],
      );
      expect(summarize(view([]), admitting), (
        count: 1,
        paused: false,
        guiding: true,
      ));
      expect(
        summarize(
          view(
            [
              {'turn': 'guide', 'guidance': 'running'},
            ],
            guidance: {'guide': 'pending'},
          ),
          admitted,
        ),
        (count: 1, paused: false, reason: null, guiding: true),
      );
      expect(summarize(view([], guidance: {'guide': 'delivered'}), admitted), (
        count: 0,
        paused: false,
        reason: null,
        guiding: false,
      ));
    });

    test('reports a paused queue', () {
      expect(
        summarize(
          view([
            {'turn': 'next'},
          ], paused: true),
          null,
        ),
        (count: 1, paused: true, reason: null, guiding: false),
      );
    });

    test('carries why a paused queue stopped sending', () {
      final items = [
        {'turn': 'next'},
      ];
      expect(
        summarize(view(items, paused: true, reason: 'interrupted'), null),
        (count: 1, paused: true, reason: 'interrupted', guiding: false),
      );
      // A reason left on a resumed queue, or one that is not text, is ignored.
      expect(summarize(view(items, reason: 'stopped'), null).reason, isNull);
      expect(
        summarize(view(items, paused: true, reason: 7), null).reason,
        isNull,
      );
    });
  });

  group('describe', () {
    String tr(String key) => switch (key) {
      'queueCount' => '{count} queued',
      'queuePaused' => 'Paused · {count}',
      'queueHaltStopped' => 'Stopped · {count}',
      'queueHaltInterrupted' => 'Interrupted · {count}',
      'queueHaltUnsent' => 'Not sent · {count}',
      'queueGuidance' => '{queue}, guidance waiting',
      _ => key,
    };

    test('states the count', () {
      expect(
        describe((count: 2, paused: false, reason: null, guiding: false), tr),
        '2 queued',
      );
    });

    test('adds waiting guidance to a paused queue', () {
      expect(
        describe((count: 1, paused: true, reason: null, guiding: true), tr),
        'Paused · 1, guidance waiting',
      );
    });

    test('names a halt instead of a pause by the user', () {
      for (final (reason, expected) in <(String?, String)>[
        (null, 'Paused · 2'),
        ('user', 'Paused · 2'),
        ('later', 'Paused · 2'),
        ('stopped', 'Stopped · 2'),
        ('interrupted', 'Interrupted · 2'),
        ('unrecorded', 'Not sent · 2'),
      ]) {
        expect(
          describe((
            count: 2,
            paused: true,
            reason: reason,
            guiding: false,
          ), tr),
          expected,
          reason: '$reason',
        );
      }
      expect(pausedKey(7), 'queuePaused');
    });
  });
}
