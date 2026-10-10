import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:sailry_mobile/features/resources/artifact/page.dart';
import 'package:sailry_mobile/features/resources/artifact/submission.dart';
import 'package:sailry_mobile/features/resources/artifact/surface.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';

// Produce --dart-define-from-file with support/artifact_view.py. It snapshots
// the package's actual declaration, never a handwritten renderer imitation.
const bundledView = String.fromEnvironment('SAILRY_ARTIFACT_VIEW');
const source = '''
<section data-title="Architecture"><p class="lead">Captured resources</p>
<pre class="mermaid">flowchart LR\n A["Node"] --> B["Client"]</pre>
<pre class="chart">{"type":"bar","title":"Requests","data":[{"label":"Local","value":3},{"label":"Remote","value":4}]}</pre>
<table><thead><tr><th>Owner</th><th>Resource</th></tr></thead><tbody><tr><td>Node</td><td>Storage</td></tr></tbody></table>
<form data-name="decisions"><fieldset data-name="storage" data-required><legend>Storage</legend>
<label><input type="radio" name="storage" value="node">Node</label>
<label><input type="radio" name="storage" value="project">Project</label></fieldset>
<fieldset data-name="note"><legend>Note</legend><textarea name="note"></textarea></fieldset>
<button type="submit">Send</button></form></section>
''';

Map<String, dynamic> artifact(int revision) => {
  'id': 'artifact',
  'session': 'session',
  'turn': 'turn',
  'package': {'name': 'fixture', 'digest': 'captured', 'settings_revision': 0},
  'title': 'HTML brief',
  'summary': 'Preview',
  'revision': revision,
  'size': source.length,
  'created_at': 1,
  'updated_at': revision,
};

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('native document stays offline and preserves answers across updates', (
    tester,
  ) async {
    expect(
      bundledView,
      isNotEmpty,
      reason: 'Provide the actual package view fixture',
    );
    final view =
        jsonDecode(utf8.decode(base64Decode(bundledView)))
            as Map<String, dynamic>;
    for (final brightness in Brightness.values) {
      var latest = 1;
      final requests = <Map<String, dynamic>>[];
      final answers = <String>[];
      final changes = ValueNotifier<Map<String, dynamic>>({
        'artifact_cursor': 1,
      });
      final actions = ArtifactActions(
        submit: (message) async {
          answers.add(message);
        },
        check: () async => true,
        writable: () => true,
        pending: () => false,
      );
      final host = HostConnection.test(
        id: 'captured-node',
        label: 'Node',
        command: (kind, data) async {
          requests.add({'kind': kind, 'data': data});
          if (kind == 'read_artifact') {
            final revision = data!['revision'] as int? ?? latest;
            return {
              'data': {
                'artifact': artifact(revision),
                'latest_revision': latest,
                'html': source.replaceAll(
                  'Captured resources',
                  'Version $revision',
                ),
                'warnings': <String>[],
                'view': view,
              },
            };
          }
          if (kind == 'list_artifact_revisions') {
            return {
              'data': {
                'items': [
                  for (var revision = latest; revision > 0; revision--)
                    artifact(revision),
                ],
                'next_before': null,
              },
            };
          }
          throw StateError('Unexpected native fixture command');
        },
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          themeAnimationDuration: Duration.zero,
          home: Align(
            alignment: Alignment.topLeft,
            child: SizedBox(
              width: 320,
              child: ArtifactPage(
                key: ValueKey(brightness),
                host: host,
                artifact: artifact(1),
                actions: actions,
                changes: changes,
              ),
            ),
          ),
        ),
      );
      await until(
        tester,
        () =>
            find.byType(WebViewWidget).evaluate().isNotEmpty &&
            find.byType(CircularProgressIndicator).evaluate().isEmpty,
      );
      final controller = WebViewController.fromPlatform(
        tester
            .widget<WebViewWidget>(find.byType(WebViewWidget))
            .platform
            .params
            .controller,
      );
      Future<Map<String, dynamic>> inspect(String expression) async =>
          jsonDecode(
                (await controller.runJavaScriptReturningResult(
                  'JSON.stringify($expression)',
                )).toString(),
              )
              as Map<String, dynamic>;
      final rendered = await inspect(
        '''({diagram:document.querySelectorAll('.document-diagram svg').length,
chart:document.querySelectorAll('.document-chart svg').length,table:document.querySelectorAll('.document-table').length,
network:[...document.scripts].filter(script=>script.src).length,csp:document.querySelector('meta[http-equiv]').content,
width:document.body.scrollWidth,viewport:innerWidth,bg:getComputedStyle(document.body).backgroundColor})''',
      );
      expect(rendered['diagram'], 1);
      expect(rendered['chart'], 1);
      expect(rendered['table'], 1);
      expect(rendered['network'], 0);
      expect(rendered['csp'], contains("connect-src 'none'"));
      expect(
        rendered['width'],
        lessThanOrEqualTo((rendered['viewport'] as num) + 1),
      );
      await controller.runJavaScript(
        "document.querySelector('input[value=node]').checked=true;document.querySelector('textarea').value='Keep this answer';document.querySelector('textarea').dispatchEvent(new Event('input',{bubbles:true}));",
      );
      await tester.pump(const Duration(milliseconds: 100));
      latest = 2;
      changes.value = {'artifact_cursor': 2};
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump();
      expect(find.text('v1'), findsOneWidget);
      final preserved = await inspect(
        "({value:document.querySelector('textarea').value,disabled:document.querySelector('textarea').disabled})",
      );
      expect(preserved['value'], 'Keep this answer');
      expect(preserved['disabled'], isTrue);
      expect(
        requests.where((call) => call['kind'] == 'read_artifact').length,
        1,
      );
      await tester.tap(find.text('v1'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('artifact-version-2')));
      await tester.pump();
      await until(
        tester,
        () =>
            find.text('v2').evaluate().isNotEmpty &&
            find.byType(CircularProgressIndicator).evaluate().isEmpty,
      );
      final refreshed = WebViewController.fromPlatform(
        tester
            .widget<WebViewWidget>(find.byType(WebViewWidget))
            .platform
            .params
            .controller,
      );
      await refreshed.runJavaScript(
        "document.querySelector('input[value=node]').checked=true;document.querySelector('textarea').value='New answer';document.querySelector('form').requestSubmit();",
      );
      await until(tester, () => answers.length == 1);
      expect(answers.single, contains('v2'));
      expect(answers.single, contains('decisions'));
      expect(answers.single, contains('Node (node)'));
      expect(answers.single, contains('New answer'));
      final surface = tester.state<ArtifactSurfaceState>(
        find.byType(ArtifactSurface),
      );
      final exported = await tester.runAsync(surface.exportHtml);
      expect(exported, contains('<svg'));
      expect(exported, isNot(contains('<script')));
      expect(exported, isNot(contains('<form')));
      expect(exported, contains('New answer'));
      expect(exported, contains('https://sailry.dev'));
      expect(exported, contains('document-brand'));
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      changes.dispose();
      actions.dispose();
      host.dispose();
    }
  });
}

Future<void> until(WidgetTester tester, bool Function() complete) async {
  final deadline = DateTime.now().add(const Duration(seconds: 45));
  while (!complete()) {
    if (DateTime.now().isAfter(deadline)) {
      throw TimeoutException('Native artifact preview did not settle');
    }
    await tester.pump(const Duration(milliseconds: 50));
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 50)),
    );
  }
}
