import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';

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
  'inputs': {
    'primary': [
      {
        'turn': 'primary',
        'message': {'text': 'Original task'},
        'canonical': true,
      },
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
  },
  'rows': {
    'primary': [
      {
        'source': {'entry': 'answer', 'index': 0},
        'draft': false,
        'part': {'kind': 'text', 'data': 'Current answer'},
        'text_offset': 0,
        'text_last': true,
        'work': false,
      },
    ],
  },
};

void main() {
  test('keeps Client groups and input identities', () {
    final view = fixture();
    final before = jsonEncode(view);
    final index = TranscriptIndex(view);
    expect(index.runs.length, 3);
    expect(index.timeline.map((run) => run['turn']), ['primary']);
    expect(index.latest, same(index.timeline.single));
    final inputs = index.inputsOf('primary');
    expect(inputs.map((input) => input.turn), [
      'primary',
      'second-guide',
      'first-guide',
    ]);
    expect(inputs[1].text, '  Guidance 中文 🙂\nOriginal spacing  ');
    expect(inputs[1].references, [
      {'target': 'project', 'label': 'Captured project'},
    ]);
    expect(inputs[1].attachments, [
      {
        'id': 'image',
        'spec': {'name': 'original.png', 'worktree': 'second-tree'},
      },
    ]);
    expect(inputs.every((input) => input.canonical), isTrue);
    expect(index.worktree(inputs[1].turn, {}), 'second-tree');
    expect(index.rowsOf('primary').single.$3['data'], 'Current answer');
    expect(jsonEncode(view), before);
  });

  test('does not restore rows omitted by Client', () {
    final view = fixture()..['timeline'] = <String>[];
    final index = TranscriptIndex(view);
    expect(index.runs, isNotEmpty);
    expect(index.timeline, isEmpty);
    expect(index.latest, isNull);
  });

  test('keeps queued input separate from joined guidance', () {
    final view = fixture();
    final page = view['snapshot']['page'];
    page['runs'].add({'turn': 'queued', 'kind': 'task', 'status': 'queued'});
    view['timeline'].add('queued');
    view['inputs']['queued'] = [
      {
        'turn': 'queued',
        'message': {'text': 'A separate task'},
        'canonical': false,
      },
    ];
    final index = TranscriptIndex(view);
    expect(index.timeline.map((run) => run['turn']), ['primary', 'queued']);
    expect(index.inputsOf('primary').length, 3);
    expect(index.inputsOf('queued').single.turn, 'queued');
    expect(index.inputsOf('queued').single.text, 'A separate task');
    expect(index.inputsOf('queued').single.canonical, isFalse);
  });

  test('stream replacement and reconnect keep one input group', () {
    final view = fixture();
    final streaming = jsonDecode(jsonEncode(view)) as Map<String, dynamic>;
    streaming['snapshot']['page']['entries'] = <Map<String, dynamic>>[];
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
    streaming['rows']['primary'][0]['source']['entry'] = 'draft';
    streaming['rows']['primary'][0]['draft'] = true;
    streaming['rows']['primary'][0]['part']['data'] = 'Streaming answer';
    final reconnected = jsonDecode(jsonEncode(view)) as Map<String, dynamic>;
    for (final projection in [streaming, view, reconnected]) {
      final index = TranscriptIndex(projection);
      expect(index.timeline.map((run) => run['turn']), ['primary']);
      expect(index.inputsOf('primary').map((input) => input.turn), [
        'primary',
        'second-guide',
        'first-guide',
      ]);
      expect(index.rowsOf('primary').length, 1);
    }
    expect(
      TranscriptIndex(streaming).rowsOf('primary').single.$3['data'],
      'Streaming answer',
    );
    expect(
      TranscriptIndex(reconnected).rowsOf('primary').single.$3['data'],
      'Current answer',
    );
  });
}
