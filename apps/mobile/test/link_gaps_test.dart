import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:sailry_mobile/content/markdown.dart';

const gap = '\u202f';

List<md.Node> parse(String source) => md.Document(
  inlineSyntaxes: [ContentLinks(images: false, files: false)],
  extensionSet: md.ExtensionSet.gitHubFlavored,
  encodeHtml: false,
).parseLines(const LineSplitter().convert(source));

Iterable<md.Element> anchors(List<md.Node> nodes) sync* {
  for (final node in nodes.whereType<md.Element>()) {
    if (node.tag == 'a') yield node;
    yield* anchors(node.children ?? []);
  }
}

List<String> links(String source) => [
  for (final link in anchors(parse(source))) link.textContent,
];

void main() {
  group('needsLinkGap', () {
    test('pads word characters', () {
      for (final char in ['交', 'か', '한', 'R', 'é', '7', '𠀋']) {
        expect(needsLinkGap(char), isTrue, reason: char);
      }
    });

    test('skips whitespace', () {
      for (final char in [' ', '\t', '\n', '\u00a0', '\u3000']) {
        expect(needsLinkGap(char), isFalse, reason: char);
      }
    });

    test('skips punctuation', () {
      for (final char in [',', '.', '(', ')', '"', '«', '—', '…']) {
        expect(needsLinkGap(char), isFalse, reason: char);
      }
      for (final char in ['，', '。', '、', '「', '」', '（', '）', '：', '！']) {
        expect(needsLinkGap(char), isFalse, reason: char);
      }
    });

    test('skips paragraph edges', () {
      expect(needsLinkGap(null), isFalse);
    });
  });

  group('content links', () {
    test('separates CJK neighbours', () {
      expect(links('交给[Reduce compaction cost](x)处理'), [
        '${gap}Reduce compaction cost$gap',
      ]);
      expect(links('𠀋[a](x)𠀋'), ['${gap}a$gap']);
    });

    test('separates Latin and digit neighbours', () {
      expect(links('see[docs](x)now, v2[notes](y)3'), [
        '${gap}docs$gap',
        '${gap}notes$gap',
      ]);
    });

    test('leaves spaced and punctuated neighbours alone', () {
      expect(links('交给 [cost](x) 处理, see\n[docs](y). （[注释](z)）'), [
        'cost',
        'docs',
        '注释',
      ]);
    });

    test('leaves paragraph edges alone', () {
      expect(links('[开始](x)中间[结束](y)'), ['开始$gap', '$gap结束']);
      expect(links('交给[a](x)\n\n[b](y)处理'), ['${gap}a', 'b$gap']);
    });

    test('separates consecutive links', () {
      expect(links('[一](x)[two](y)[3](z)'), [
        '一$gap',
        '${gap}two$gap',
        '${gap}3',
      ]);
    });

    test('leaves image links alone', () {
      final link = anchors(parse('交[![logo](logo.png)](x)给')).single;
      expect(link.children, hasLength(1));
    });
  });
}
