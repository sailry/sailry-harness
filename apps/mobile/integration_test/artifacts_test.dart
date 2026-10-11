import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:sailry_mobile/features/resources/artifact/page.dart';
import 'package:sailry_mobile/features/resources/artifact/submission.dart';
import 'package:sailry_mobile/features/resources/artifact/surface.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';

import 'support/webview.dart';

// Produce --dart-define-from-file with support/artifact_view.py. It snapshots
// the package's actual declaration, never a handwritten renderer imitation.
const bundledView = String.fromEnvironment('SAILRY_ARTIFACT_VIEW');
const source = '''
<section id="architecture" data-title="Architecture"><p class="lead">Captured resources</p>
<a href="#architecture">Architecture reference</a>
<pre class="mermaid">flowchart LR\n A["Node"] --> B["Client"]</pre>
<pre class="chart">{"type":"bar","title":"Requests","data":[{"label":"Local","value":3},{"label":"Remote","value":4}]}</pre>
<table><thead><tr><th>Owner</th><th>Resource</th></tr></thead><tbody><tr><td>Node</td><td>Storage</td></tr></tbody></table>
<div id="alignment-fixture"><label><span>Caption</span><input type="checkbox"></label>
<label><span>Caption</span><input type="text"></label>
<label><span>Caption</span><select><option>Option</option></select></label></div>
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
                'html': source
                    .replaceAll('Captured resources', 'Version $revision')
                    .replaceAll(
                      '"type":"bar"',
                      revision == 3 ? '"type":"unsupported"' : '"type":"bar"',
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
      Future<Map<String, dynamic>> inspect(String expression) =>
          readObject(controller, expression);
      final rendered = await inspect(
        '''({diagram:document.querySelectorAll('.document-diagram svg').length,
diagramFiltered:[...document.querySelectorAll('.document-diagram svg,.document-diagram svg *')].some(node=>getComputedStyle(node).filter!=='none'),
diagramFill:getComputedStyle(document.querySelector('.document-diagram .node rect')).fill,
diagramStroke:getComputedStyle(document.querySelector('.document-diagram .node rect')).stroke,
diagramText:getComputedStyle(document.querySelector('.document-diagram text')).fill,
foreground:getComputedStyle(document.body).color,
chart:document.querySelectorAll('.document-chart svg').length,table:document.querySelectorAll('.document-table').length,
network:[...document.scripts].filter(script=>script.src).length,csp:document.querySelector('meta[http-equiv]').content,
width:document.body.scrollWidth,viewport:innerWidth,bg:getComputedStyle(document.body).backgroundColor})''',
      );
      expect(rendered['diagram'], 1);
      expect(rendered['diagramFill'], 'none');
      expect(rendered['diagramFiltered'], isFalse);
      expect(rendered['diagramStroke'], rendered['foreground']);
      expect(rendered['diagramText'], rendered['foreground']);
      expect(rendered['chart'], 1);
      expect(rendered['table'], 1);
      expect(rendered['network'], 0);
      expect(rendered['csp'], contains("connect-src 'none'"));
      expect(
        rendered['width'],
        lessThanOrEqualTo((rendered['viewport'] as num) + 1),
      );
      final alignment = await inspect('''(() => {
const fixture=document.getElementById('alignment-fixture');
const rows=[...fixture.querySelectorAll('label')];
try {
 return {rows:rows.map(label=>{
  const caption=label.querySelector('span').getBoundingClientRect(),control=label.querySelector('input,select').getBoundingClientRect();
  const style=getComputedStyle(label);
  return {marked:label.classList.contains('document-control'),display:style.display,align:style.alignItems,center:Math.abs(caption.y+caption.height/2-control.y-control.height/2)};
 })};
} finally {fixture.remove();}
})()''');
      for (final row in (alignment['rows'] as List).cast<Map>()) {
        expect(row['marked'], isTrue);
        expect(row['display'], 'flex');
        expect(row['align'], 'center');
        expect(row['center'], lessThanOrEqualTo(1));
      }
      final anchor = await inspect('''(() => {
const section = document.querySelector('main section');
const targets = [];
const original = Element.prototype.scrollIntoView;
Element.prototype.scrollIntoView = function() { targets.push(this.id); };
try { document.querySelector('main a[href="#architecture"]').click(); }
finally { Element.prototype.scrollIntoView = original; }
return {id:section.id,targets};
})()''');
      expect(anchor['id'], 'architecture');
      expect(anchor['targets'], ['architecture']);
      for (var repetition = 0; repetition < 2; repetition++) {
        await controller.runJavaScript(
          "document.querySelector('form').requestSubmit();",
        );
        await until(
          tester,
          () => find.text(tr('artifactFormRequired')).evaluate().isNotEmpty,
        );
        expect(find.byType(WebViewWidget), findsOneWidget);
        await until(
          tester,
          () => find.text(tr('artifactFormRequired')).evaluate().isEmpty,
        );
      }
      await controller.runJavaScript('''(() => {
const field = document.querySelector('fieldset[data-name="storage"]');
field.dataset.name = '';
try { document.querySelector('form').requestSubmit(); }
finally { field.dataset.name = 'storage'; }
})()''');
      await until(
        tester,
        () => find.text(tr('artifactFormInvalid')).evaluate().isNotEmpty,
      );
      expect(find.byType(WebViewWidget), findsOneWidget);
      await until(
        tester,
        () => find.text(tr('artifactFormInvalid')).evaluate().isEmpty,
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
      latest = 3;
      changes.value = {'artifact_cursor': 3};
      await tester.pump(const Duration(milliseconds: 500));
      await tester.tap(find.text('v2'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('artifact-version-3')));
      await until(
        tester,
        () =>
            find.text('v3').evaluate().isNotEmpty &&
            find.byType(CircularProgressIndicator).evaluate().isEmpty &&
            find.text(tr('artifactRenderFailed')).evaluate().isNotEmpty,
      );
      final fallback = WebViewController.fromPlatform(
        tester
            .widget<WebViewWidget>(find.byType(WebViewWidget))
            .platform
            .params
            .controller,
      );
      final degraded = await readObject(fallback, '''({
diagram:document.querySelectorAll('.document-diagram svg').length,
diagramFiltered:[...document.querySelectorAll('.document-diagram svg,.document-diagram svg *')].some(node=>getComputedStyle(node).filter!=='none'),
diagramFill:getComputedStyle(document.querySelector('.document-diagram .node rect')).fill,
diagramStroke:getComputedStyle(document.querySelector('.document-diagram .node rect')).stroke,
diagramText:getComputedStyle(document.querySelector('.document-diagram text')).fill,
foreground:getComputedStyle(document.body).color,
chart:document.querySelectorAll('.document-chart svg').length,
source:document.querySelector('.document-chart pre')?.textContent,
form:!!document.querySelector('form')
})''');
      expect(degraded['diagram'], 1);
      expect(degraded['chart'], 0);
      expect(degraded['source'], contains('"type":"unsupported"'));
      expect(degraded['form'], isTrue);
      expect(find.byType(WebViewWidget), findsOneWidget);
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
