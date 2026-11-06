import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';

Map<String, dynamic> view(
  List<Map<String, dynamic>> items, {
  Map<String, dynamic> guidance = const {},
  bool paused = false,
}) => {
  'snapshot': {
    'page': {
      'queue': {'items': items, 'paused': paused},
    },
  },
  'guidance': guidance,
};

void main() {
  group('summarize', () {
    test('is empty without queued input', () {
      expect(summarize({}, null), (count: 0, paused: false, guiding: false));
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
      expect(state, (count: 2, paused: false, guiding: true));
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
        (count: 1, paused: false, guiding: true),
      );
      expect(summarize(view([], guidance: {'guide': 'delivered'}), admitted), (
        count: 0,
        paused: false,
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
        (count: 1, paused: true, guiding: false),
      );
    });
  });
}
