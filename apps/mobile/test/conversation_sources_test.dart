import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/sources.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

void main() {
  Future<void> mount(WidgetTester tester, Map<String, dynamic> entry) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Scaffold(
          body: SingleChildScrollView(child: EntrySources(entry: entry)),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('citations open original destinations', (tester) async {
    const channel = MethodChannel('plugins.flutter.io/url_launcher');
    final opened = <String>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
      call,
    ) async {
      if (call.method == 'launch') {
        opened.add((call.arguments as Map)['url'] as String);
        return true;
      }
      return false;
    });
    addTearDown(() {
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      );
    });
    await mount(tester, {
      'citations': [
        {'title': 'Release details', 'uri': 'https://example.com/a_(b)'},
        {'title': null, 'uri': 'https://example.com/other'},
      ],
    });
    expect(find.text(tr('conversationSources')), findsOneWidget);
    expect(find.text('[1]'), findsOneWidget);
    expect(find.text('[2]'), findsOneWidget);
    expect(find.text('https://example.com/other'), findsOneWidget);
    await tester.tap(find.text('Release details'));
    await tester.pumpAndSettle();
    expect(opened, ['https://example.com/a_(b)']);
  });

  testWidgets('suggestions preserve links and exclude executable content', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 900);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await mount(tester, {
      'citations': [
        {'title': 'Unsafe source', 'uri': 'javascript:alert(1)'},
      ],
      'search_suggestions': '''
        <style>.chip { color: red }</style><script>doNotRender()</script>
        <div>Suggested <strong>searches</strong></div>
        <a href="https://example.com/search?a=1&amp;b=2"><span>Search &amp; learn</span></a>
        <a href="file:///private/data">Local file</a>
        <a href="mailto:contact@example.com">Email</a>
        <a href="https:missing-host">Malformed web URL</a>
        <iframe src="https://example.com/embed">Embedded page</iframe>
      ''',
    });
    expect(find.text(tr('conversationSearchSuggestions')), findsOneWidget);
    expect(find.text('Suggested searches'), findsOneWidget);
    expect(find.text('Search & learn'), findsOneWidget);
    expect(find.textContaining('doNotRender'), findsNothing);
    expect(find.textContaining('color: red'), findsNothing);
    expect(find.text('Embedded page'), findsNothing);
    for (final label in [
      'Unsafe source',
      'Local file',
      'Email',
      'Malformed web URL',
    ]) {
      final button = find.ancestor(
        of: find.text(label),
        matching: find.byType(TextButton),
      );
      expect(tester.widget<TextButton>(button).onPressed, isNull);
    }
    final valid = find.ancestor(
      of: find.text('Search & learn'),
      matching: find.byType(TextButton),
    );
    expect(tester.widget<TextButton>(valid).onPressed, isNotNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('no controls for empty metadata', (tester) async {
    await mount(tester, {
      'citations': <Map<String, dynamic>>[],
      'search_suggestions': '<style>.chip { color: red }</style>',
    });
    expect(find.byType(TextButton), findsNothing);
    expect(find.text(tr('conversationSources')), findsNothing);
    expect(find.text(tr('conversationSearchSuggestions')), findsNothing);
  });

  testWidgets('answer and metadata-only sources remain visible', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    const answer = 'A grounded answer with **unchanged Markdown**';
    final projection = fixture.view(
      entries: [
        {
          ...fixture.entry('answer', 'assistant', answer),
          'citations': [
            {'title': 'Answer source', 'uri': 'https://example.com/source'},
          ],
        },
        {
          ...fixture.entry('metadata', 'assistant', ''),
          'parts': <Map<String, dynamic>>[],
          'search_suggestions':
              '<a href="https://example.com/search">Related search</a>',
        },
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
    expect(find.text('Answer source'), findsOneWidget);
    expect(find.text('Related search'), findsOneWidget);
    expect(
      tester.widget<MarkdownContent>(find.byType(MarkdownContent)).data,
      '$answer [1](<https://example.com/source>)',
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });
}
