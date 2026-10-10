import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:sailry_mobile/content/math.dart';

// These payloads mirror Desktop's parse/math/tests.rs acceptance cases.
const density =
    r'f_X(x)=\frac{1}{\sigma\sqrt{2\pi}}\exp\left(-\frac{(x-\mu)^2}{2\sigma^2}\right)';
const bayes = r'P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)}';

List<md.Node> parse(String source) => md.Document(
  blockSyntaxes: const [MathBlockSyntax()],
  inlineSyntaxes: [MathInlineSyntax(), _LiteralLinks()],
  extensionSet: md.ExtensionSet.gitHubFlavored,
).parseLines(const LineSplitter().convert(source));

Iterable<md.Element> elements(List<md.Node> nodes) sync* {
  for (final node in nodes.whereType<md.Element>()) {
    yield node;
    yield* elements(node.children ?? []);
  }
}

List<md.Element> formulas(String source) => elements(
  parse(source),
).where((node) => node.tag.startsWith('sailry-math-')).toList();

class _LiteralLinks extends md.LinkSyntax {
  @override
  md.Node createNode(
    String destination,
    String? title, {
    required List<md.Node> Function() getChildren,
  }) => super.createNode(
    destination,
    title,
    getChildren: () => getChildren().map(literalLinkMath).toList(),
  );
}

void main() {
  group('math grammar', () {
    test('recognizes normal and Bayes formulas', () {
      final source = 'Normal \\($density\\).\n\n\\[\n$bayes\n\\]\n';
      final math = formulas(source);
      expect(math.map((node) => node.textContent), [density, bayes]);
      expect(math.map((node) => node.tag), [
        'sailry-math-inline',
        'sailry-math-block',
      ]);
    });

    test('preserves Unicode and TeX payloads', () {
      const aligned = r'\begin{aligned}a &= b \\ c &= d\end{aligned}';
      final source =
          '中文 \\(\\text{price \$5 & *literal*}\\) and \$y\$.\r\n\r\n\\[\r\n$aligned\r\n\\]\r\n\r\n尾部\r\n';
      expect(formulas(source).map((node) => node.textContent), [
        r'\text{price $5 & *literal*}',
        'y',
        aligned,
      ]);
    });

    test('keeps code links and escapes literal', () {
      const source =
          r'''`\(code\)` and \\(escaped\\) and [\(label\)](https://example.com/\(path\)) and $\text{\(nested\)}$.

```latex
\[fenced\]
```

    \(indented\)

[ref]: https://example.com/\(reference\)
''';
      final nodes = elements(parse(source)).toList();
      expect(formulas(source).map((node) => node.textContent), [
        r'\text{\(nested\)}',
      ]);
      final link = nodes.singleWhere((node) => node.tag == 'a');
      expect(link.textContent, '(label)');
      expect(link.attributes['href'], 'https://example.com/(path)');
      expect(
        nodes
            .where((node) => node.tag == 'code')
            .map((node) => node.textContent),
        [r'\(code\)', '${r'\[fenced\]'}\n', '${r'\(indented\)'}\n'],
      );
    });

    test('protects references and image labels', () {
      const source = r'''[\(label\)][ref] and ![\(alt\)](image.png)

[ref]: https://example.com/\(reference\)
''';
      expect(formulas(source), isEmpty);
      final link = elements(
        parse(source),
      ).firstWhere((node) => node.tag == 'a');
      expect(link.textContent, '(label)');
      expect(link.attributes['href'], 'https://example.com/(reference)');
      expect(formulas(r'\(unfinished [\)](https://example.com)'), isEmpty);
      expect(formulas(r'\[unfinished [link](https://example.com/\])'), isEmpty);
    });

    test('preserves quote and list containers', () {
      for (final source in ['> \\[\n> x^2\n> \\]\n', '- \\[\n  x^2\n  \\]\n']) {
        final nodes = elements(parse(source)).toList();
        expect(formulas(source).single.textContent, 'x^2');
        expect(
          nodes.any(
            (node) =>
                node.tag == (source.startsWith('>') ? 'blockquote' : 'li'),
          ),
          isTrue,
        );
      }
    });

    test('keeps unfinished and empty delimiters literal', () {
      for (final source in [
        r'\(unfinished',
        r'\[unfinished',
        r'\(\)',
        r'\[ \]',
        '\\[\n\nx\n\\]',
        r'\(unfinished `code \)`',
        r'\[unfinished [link](https://example.com/\])',
      ]) {
        expect(formulas(source), isEmpty, reason: source);
      }
    });

    test('accepts dollars and ignores escaped closers', () {
      expect(formulas(r'$x$ and $$y^2$$').map((node) => node.textContent), [
        'x',
        'y^2',
      ]);
      expect(formulas('\$\$\nx^2\n\$\$').single.tag, 'sailry-math-block');
      expect(formulas(r'\[a \\] b\]').single.textContent, r'a \\] b');
      expect(formulas(r'\$x\$ and $ spaced $'), isEmpty);
    });
  });
}
