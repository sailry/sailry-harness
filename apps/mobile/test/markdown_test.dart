import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

const source = '''
## Comparison

Normal paragraph with **bold** and [a link](https://example.com).

| 项目 | 版本或型号 | 正式日期 |
| --- | --- | --- |
| NVIDIA | Blackwell B200, GB200 | 2024 年 3 月 18 日公布 |
| macOS | macOS Sonoma 14.5 | 2024 年 5 月 13 日发布 |

- First item
- Second item

```rust
let message = "A code example";
```
''';

void main() {
  testWidgets('image links preserve normal links and code', (tester) async {
    const source = '''
[Download](assets/image.png)

![Explicit](assets/other.png)

[Website](https://example.test/photo.png) [Email](mailto:a@example.test)

[Document](notes.txt) [Remote file](file://server/image.png)

`[Code](code.png)`

```text
[Example](example.png)
```
''';
    final images = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MarkdownContent(
            source,
            imageBuilder: (uri, _, _) {
              images.add(uri.toString());
              return const SizedBox(height: 10);
            },
          ),
        ),
      ),
    );
    expect(images, ['assets/image.png', 'assets/other.png']);
    final body = tester.widget<MarkdownBody>(find.byType(MarkdownBody));
    expect(body.data, source);
    final html = md.markdownToHtml(
      source,
      inlineSyntaxes: body.inlineSyntaxes!,
    );
    expect(
      html,
      contains('<a href="https://example.test/photo.png">Website</a>'),
    );
    expect(html, contains('<a href="mailto:a@example.test">Email</a>'));
    expect(html, contains('<a href="notes.txt">Document</a>'));
    expect(html, contains('<a href="file://server/image.png">Remote file</a>'));
    expect(html, contains('<code>[Code](code.png)</code>'));
    expect(html, contains('[Example](example.png)'));
    expect(tester.takeException(), isNull);
  });

  for (final width in [320.0, 390.0]) {
    for (final brightness in Brightness.values) {
      testWidgets('scrollable tables at $width in $brightness', (tester) async {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = Size(width, 900);
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        await tester.pumpWidget(
          MaterialApp(
            theme: SailryTheme.of(brightness),
            home: const Scaffold(
              body: SingleChildScrollView(
                padding: EdgeInsets.all(20),
                child: MarkdownContent(source),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final table = find.byType(Table);
        expect(table, findsOneWidget);
        expect(tester.widget<Table>(table).children, hasLength(3));
        expect(tester.getSize(table).width, greaterThan(width));
        final horizontal = find.ancestor(
          of: table,
          matching: find.byWidgetPredicate(
            (widget) =>
                widget is SingleChildScrollView &&
                widget.scrollDirection == Axis.horizontal,
          ),
        );
        expect(horizontal, findsOneWidget);
        expect(tester.getSize(horizontal).width, width - 40);
        final scroll = tester
            .widget<SingleChildScrollView>(horizontal)
            .controller!;
        await tester.drag(horizontal, const Offset(-300, 0));
        await tester.pumpAndSettle();
        expect(scroll.offset, greaterThan(0));
        expect(find.text('Comparison', findRichText: true), findsWidgets);
        expect(tester.takeException(), isNull);
      });
    }
  }

  testWidgets('table fragments stay within their message', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    final projection = fixture.view(
      entries: [
        {
          'id': 'answer',
          'turn': 'turn',
          'author': 'assistant',
          'parts': [
            for (final rune in source.runes)
              {'kind': 'text', 'data': String.fromCharCode(rune)},
          ],
        },
        fixture.entry('other', 'assistant', 'Separate message'),
      ],
    );
    await fixture.mount(
      tester,
      Scaffold(
        body: SingleChildScrollView(
          child: LiveTimeline(
            view: projection,
            session: fixture.session(),
            host: host,
            command: (_, _) async => {},
          ),
        ),
      ),
      app,
    );
    await tester.pumpAndSettle();
    expect(find.byType(Table), findsOneWidget);
    expect(find.byType(MarkdownContent), findsNWidgets(2));
    expect(
      tester
          .widgetList<MarkdownContent>(find.byType(MarkdownContent))
          .first
          .data,
      source,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });
}
