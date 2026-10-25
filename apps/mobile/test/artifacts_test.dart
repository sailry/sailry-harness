import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/artifacts.dart';
import 'package:sailry_mobile/features/resources/artifact/content.dart';

Map<String, dynamic> summary(int revision, {String kind = 'document'}) => {
  'id': 'artifact',
  'kind': kind,
  'session': 'session',
  'turn': 'turn',
  'package': {'name': 'owner', 'digest': 'frozen', 'settings_revision': 0},
  'title': 'Architecture',
  'summary': 'Shared resources',
  'revision': revision,
  'size': 100,
  'created_at': 1,
  'updated_at': revision,
};
Map<String, dynamic> content(int revision, {int? latest}) => {
  'artifact': summary(revision),
  'latest_revision': latest ?? revision,
  'html': '<h1>Version $revision</h1>',
  'warnings': <String>[],
  'view': {
    'template':
        '<html><head></head><body><main id="brief"></main></body></html>',
    'styles': ['main{color:var(--document-foreground)}'],
    'scripts': [
      'window.sailryDocument={mount:()=>{},setWritable:()=>{},retheme:()=>{},submitted:()=>{},readonlyExport:()=>"<main><p>Exported</p></main>"}',
    ],
    'libraries': <String>[],
  },
};
Map<String, dynamic> publication(int revision, {String kind = 'document'}) => {
  'turn': 'turn',
  'source': {'entry': 'tool-$revision', 'index': 0},
  'content': {
    'version': 1,
    'blocks': [
      {'kind': 'artifact', 'artifact': summary(revision, kind: kind)},
    ],
  },
};
Map<String, dynamic> submission(int revision) => {
  'kind': 'submit',
  'artifact': 'artifact',
  'session': 'session',
  'revision': revision,
  'form': 'choices',
  'fields': [
    {
      'name': 'storage',
      'label': 'Storage',
      'values': [
        {'label': 'Node', 'value': 'node'},
      ],
    },
  ],
};

void main() {
  test('publications retain revisions and captured ownership', () {
    final other = publication(3);
    other['content']['blocks'][0]['artifact']['session'] = 'other';
    expect(
      publishedArtifacts(
        [publication(1), publication(1), publication(2), other],
        'turn',
        'session',
      ),
      [summary(1), summary(2)],
    );
    expect(
      publishedArtifacts([publication(1)], 'other-turn', 'session'),
      isEmpty,
    );
    expect(
      publishedArtifacts([publication(1)], 'turn', 'fork', origin: 'session'),
      [summary(1)],
    );
    expect(publishedArtifacts([publication(1)], 'turn', 'fork'), isEmpty);
    expect(
      publishedArtifacts([publication(1)], 'turn', 'fork', origin: 'other'),
      isEmpty,
    );
  });
  test(
    'publication kinds survive JSON restoration and reject invalid blocks',
    () {
      for (final kind in ['document', 'preview']) {
        final restored =
            jsonDecode(jsonEncode(publication(1, kind: kind)))
                as Map<String, dynamic>;
        expect(publishedArtifacts([restored], 'turn', 'session'), [
          summary(1, kind: kind),
        ]);
        final duplicate =
            jsonDecode(jsonEncode(restored)) as Map<String, dynamic>;
        expect(publishedArtifacts([restored, duplicate], 'turn', 'session'), [
          summary(1, kind: kind),
        ]);
        for (var change = 0; change < 7; change++) {
          final invalid =
              jsonDecode(jsonEncode(restored)) as Map<String, dynamic>;
          final block = invalid['content']['blocks'][0];
          final artifact = block['artifact'];
          switch (change) {
            case 0:
              block['kind'] = 'file';
            case 1:
              block.remove('artifact');
              block.addAll(artifact);
              block['kind'] = 'artifact';
            case 2:
              artifact['session'] = 'other';
            case 3:
              artifact['turn'] = 'other';
            case 4:
              artifact['revision'] = 0;
            case 5:
              artifact['id'] = '';
            case 6:
              block.remove('artifact');
          }
          expect(publishedArtifacts([invalid], 'turn', 'session'), isEmpty);
        }
      }
    },
  );
  test('form answers include capture and original choice values', () {
    expect(
      artifactAnswers(submission(2), summary(2), 'Architecture v2 (artifact)'),
      'Architecture v2 (artifact)\n- Storage: Node (node)',
    );
    expect(
      () => artifactAnswers(submission(1), summary(2), 'header'),
      throwsFormatException,
    );
    expect(
      () => artifactAnswers(
        {...submission(2), 'session': 'other'},
        summary(2),
        'header',
      ),
      throwsFormatException,
    );
  });
  test('export retains rendered content without executable scripts', () {
    final result = exportedArtifact(
      '<main><svg><path/></svg><script>bad()</script><p>Answer</p></main>',
      content(1),
      {'foreground': '#123456'},
      logo: '<svg id="logo"></svg>',
      qr: '<svg id="qr"></svg>',
    );
    expect(result, contains('<svg>'));
    expect(result, contains('Answer'));
    expect(result, isNot(contains('<script')));
    expect(result, contains('https://sailry.dev'));
    expect(result, contains('id="qr"'));
    expect(result, contains('--document-foreground:#123456'));
  });
}
