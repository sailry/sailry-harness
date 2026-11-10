import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';

Map<String, dynamic> row(String entry, String data) => {
  'source': {'entry': entry, 'index': 0},
  'draft': false,
  'part': {'kind': 'text', 'data': data},
  'text_offset': 0,
  'text_last': true,
  'work': false,
};

Map<String, dynamic> fixture() => {
  'snapshot': {
    'page': {
      'runs': [
        {'turn': 'primary', 'kind': 'task', 'worktree': 'primary-tree'},
        {
          'turn': 'first-guide',
          'kind': 'task',
          'joined': 'primary',
          'worktree': 'first-tree',
        },
        {
          'turn': 'second-guide',
          'kind': 'task',
          'joined': 'primary',
          'worktree': 'second-tree',
        },
      ],
      'entries': [
        {
          'id': 'early',
          'turn': 'primary',
          'author': 'assistant',
          'parts': [
            {'kind': 'text', 'data': 'Early answer'},
          ],
        },
        {
          'id': 'answer',
          'turn': 'primary',
          'author': 'assistant',
          'parts': [
            {'kind': 'text', 'data': 'Current answer'},
          ],
        },
      ],
    },
  },
  'timeline': ['primary'],
  'segments': {
    'primary': [
      {
        'inputs': [
          {
            'turn': 'primary',
            'message': {'text': 'Original task'},
            'canonical': true,
          },
        ],
        'rows': [row('early', 'Early answer')],
      },
      {
        'inputs': [
          {
            'turn': 'second-guide',
            'message': {
              'text': '  Guidance 中文 🙂\nOriginal spacing  ',
              'references': [
                {'target': 'project', 'label': 'Captured project'},
              ],
            },
            'attachments': [
              {
                'id': 'image',
                'spec': {'name': 'original.png', 'worktree': 'second-tree'},
              },
            ],
            'canonical': true,
          },
          {
            'turn': 'first-guide',
            'message': {'text': 'Later canonical input'},
            'canonical': true,
          },
        ],
        'rows': [row('answer', 'Current answer')],
      },
    ],
  },
};

void main() {
  test('keeps Client segments and input identities', () {
    final view = fixture();
    final before = jsonEncode(view);
    final index = TranscriptIndex(view);
    expect(index.runs.length, 3);
    expect(index.timeline.map((run) => run['turn']), ['primary']);
    expect(index.latest, same(index.timeline.single));
    final segments = index.segmentsOf('primary');
    expect(segments.length, 2);
    expect(segments.first.inputs.map((input) => input.turn), ['primary']);
    expect(segments.first.rows.single.$3['data'], 'Early answer');
    final inputs = segments.last.inputs;
    expect(inputs.map((input) => input.turn), ['second-guide', 'first-guide']);
    expect(inputs.first.text, '  Guidance 中文 🙂\nOriginal spacing  ');
    expect(inputs.first.references, [
      {'target': 'project', 'label': 'Captured project'},
    ]);
    expect(inputs.first.attachments, [
      {
        'id': 'image',
        'spec': {'name': 'original.png', 'worktree': 'second-tree'},
      },
    ]);
    expect(inputs.every((input) => input.canonical), isTrue);
    expect(index.worktree(inputs.first.turn, {}), 'second-tree');
    expect(segments.last.rows.single.$3['data'], 'Current answer');
    expect(index.segmentsOf('first-guide'), isEmpty);
    expect(jsonEncode(view), before);
  });

  test('reads waiting children from the latest Client segment', () {
    final view = jsonDecode(jsonEncode(fixture())) as Map<String, dynamic>;
    expect(TranscriptIndex(view).waitingOf('primary'), isNull);
    final segments = view['segments']['primary'] as List;
    segments.first['waiting'] = 3;
    expect(TranscriptIndex(view).waitingOf('primary'), isNull);
    segments.last['waiting'] = 2;
    final index = TranscriptIndex(view);
    expect(index.waitingOf('primary'), 2);
    expect(index.waitingOf('first-guide'), isNull);
  });

  test('does not restore rows omitted by Client', () {
    final view = fixture()..['timeline'] = <String>[];
    final index = TranscriptIndex(view);
    expect(index.runs, isNotEmpty);
    expect(index.timeline, isEmpty);
    expect(index.latest, isNull);
  });

  test('keeps queued input separate from joined guidance', () {
    final view = jsonDecode(jsonEncode(fixture())) as Map<String, dynamic>;
    final page = view['snapshot']['page'];
    page['runs'].add({'turn': 'queued', 'kind': 'task', 'status': 'queued'});
    view['timeline'].add('queued');
    view['segments']['queued'] = [
      {
        'inputs': [
          {
            'turn': 'queued',
            'message': {'text': 'A separate task'},
            'canonical': false,
          },
        ],
        'rows': [],
      },
    ];
    final index = TranscriptIndex(view);
    expect(index.timeline.map((run) => run['turn']), ['primary', 'queued']);
    expect(
      index.segmentsOf('primary').expand((segment) => segment.inputs).length,
      3,
    );
    final queued = index.segmentsOf('queued').single;
    expect(queued.inputs.single.turn, 'queued');
    expect(queued.inputs.single.text, 'A separate task');
    expect(queued.inputs.single.canonical, isFalse);
    expect(queued.rows, isEmpty);
  });

  test('stream replacement and reconnect keep the same segments', () {
    final view = fixture();
    final streaming = jsonDecode(jsonEncode(view)) as Map<String, dynamic>;
    (streaming['snapshot']['page']['entries'] as List).removeLast();
    streaming['snapshot']['drafts'] = [
      {
        'id': 'draft',
        'turn': 'primary',
        'author': 'assistant',
        'parts': [
          {'kind': 'text', 'data': 'Streaming answer'},
        ],
      },
    ];
    final streamed = streaming['segments']['primary'][1]['rows'][0];
    streamed['source']['entry'] = 'draft';
    streamed['draft'] = true;
    streamed['part']['data'] = 'Streaming answer';
    final reconnected = jsonDecode(jsonEncode(view)) as Map<String, dynamic>;
    for (final projection in [streaming, view, reconnected]) {
      final index = TranscriptIndex(projection);
      expect(index.timeline.map((run) => run['turn']), ['primary']);
      final segments = index.segmentsOf('primary');
      expect(
        segments.expand((segment) => segment.inputs).map((input) => input.turn),
        ['primary', 'second-guide', 'first-guide'],
      );
      expect(segments.map((segment) => segment.rows.length), [1, 1]);
    }
    expect(
      TranscriptIndex(
        streaming,
      ).segmentsOf('primary').last.rows.single.$3['data'],
      'Streaming answer',
    );
    expect(
      TranscriptIndex(
        reconnected,
      ).segmentsOf('primary').last.rows.single.$3['data'],
      'Current answer',
    );
  });
}
